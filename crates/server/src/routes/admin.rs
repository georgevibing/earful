//! Admin API. Every handler except login takes the [`Admin`] extractor.

use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter, types::Value as SqlValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{ClientIp, IdPath, JsonBody};
use crate::{
    auth::{self, Admin, SESSION_COOKIE},
    crypto,
    db::now,
    embed::Embedder,
    error::{AppError, AppResult},
    models::{Feedback, KINDS, PROJECT_TYPES, Project, STATUSES, slugify, valid_slug},
    ratelimit::{self, Rule},
    search::{self, Mode},
    state::AppState,
    tokens::{self, Scope},
};

const MAX_PROJECT_TITLE: usize = 120;
const MAX_DESCRIPTION: usize = 2000;
const MAX_NOTE: usize = 10_000;
const MAX_BULK: usize = 500;
const SIMILAR: usize = 5;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/projects", get(list_projects).post(create_project))
        .route(
            "/projects/{id}",
            get(get_project)
                .patch(update_project)
                .delete(delete_project),
        )
        .route("/projects/{id}/rotate-key", post(rotate_key))
        .route("/projects/{id}/export", get(export))
        .route("/feedback", get(list_feedback))
        .route("/feedback/bulk", post(bulk))
        .route(
            "/feedback/{id}",
            get(get_feedback)
                .patch(update_feedback)
                .delete(delete_feedback),
        )
        .route("/stats", get(stats))
        .route("/reindex", post(reindex))
        .route("/tokens", get(list_tokens).post(create_token))
        .route("/tokens/{id}", axum::routing::delete(delete_token))
}

fn is_unique_violation(e: &rusqlite::Error) -> bool {
    matches!(e, rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation)
}

fn new_ingest_key() -> String {
    format!("fhk_{}", crypto::random_hex(16))
}

// ---------------------------------------------------------------------------
// Auth

#[derive(Deserialize)]
struct LoginBody {
    password: String,
}

async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    jar: CookieJar,
    JsonBody(body): JsonBody<LoginBody>,
) -> AppResult<(CookieJar, StatusCode)> {
    if !auth::same_origin(&headers) {
        return Err(AppError::Forbidden(
            "bad_origin",
            "cross origin request rejected".into(),
        ));
    }
    let cfg = state.cfg.clone();
    // Only failed attempts are counted, so a correct password never locks you out.
    // The attempt is recorded together with the check (atomically, under the DB
    // lock) and removed again on success; recording after the slow argon2 check
    // would let concurrent guesses all pass the limit.
    let attempt = state
        .db
        .call(move |c| {
            let rules = [
                Rule {
                    bucket: "login",
                    ip_hash: Some(&ip),
                    window_secs: cfg.limits.login_window_secs,
                    limit: cfg.limits.logins_per_ip_window,
                },
                Rule {
                    bucket: "login",
                    ip_hash: None,
                    window_secs: 3600,
                    limit: 30,
                },
            ];
            let ts = now();
            if let Err(retry_after) = ratelimit::check(c, ts, &rules)? {
                return Ok(Err(retry_after));
            }
            ratelimit::record(c, "login", &ip, ts)?;
            Ok(Ok(c.last_insert_rowid()))
        })
        .await?;
    let attempt = attempt.map_err(|retry_after| AppError::RateLimited { retry_after })?;

    let hash = state.cfg.admin_password_hash.clone();
    let ok =
        tokio::task::spawn_blocking(move || auth::verify_password(hash.as_deref(), &body.password))
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    if !ok {
        return Err(AppError::InvalidCredentials);
    }
    state
        .db
        .call(move |c| {
            c.execute("DELETE FROM rate_events WHERE rowid = ?1", params![attempt])?;
            Ok(())
        })
        .await?;
    let token = auth::create_session(&state).await?;
    Ok((
        jar.add(auth::session_cookie(&state, token)),
        StatusCode::NO_CONTENT,
    ))
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> AppResult<(CookieJar, StatusCode)> {
    if !auth::same_origin(&headers) {
        return Err(AppError::Forbidden(
            "bad_origin",
            "cross origin request rejected".into(),
        ));
    }
    if let Some(token) = jar.get(SESSION_COOKIE).map(|c| c.value().to_string()) {
        auth::destroy_session(&state, &token).await?;
    }
    Ok((jar.add(auth::clear_cookie(&state)), StatusCode::NO_CONTENT))
}

async fn me(_: Admin) -> Json<Value> {
    Json(json!({ "ok": true }))
}

// ---------------------------------------------------------------------------
// Projects

#[derive(Serialize, Default, Clone)]
pub(crate) struct ProjectStats {
    pub(crate) total: i64,
    pub(crate) open: i64,
    pub(crate) new: i64,
    pub(crate) bugs: i64,
    pub(crate) feedback: i64,
    pub(crate) spam: i64,
    pub(crate) last_at: Option<i64>,
}

#[derive(Serialize)]
pub(crate) struct ProjectOut {
    #[serde(flatten)]
    pub(crate) project: Project,
    pub(crate) stats: ProjectStats,
}

const STATS_SELECT: &str =
    "COALESCE(SUM(status != 'spam'), 0), COALESCE(SUM(status IN ('new', 'triaged')), 0),
     COALESCE(SUM(status = 'new'), 0), COALESCE(SUM(kind = 'bug' AND status != 'spam'), 0),
     COALESCE(SUM(kind = 'feedback' AND status != 'spam'), 0), COALESCE(SUM(status = 'spam'), 0),
     MAX(CASE WHEN status != 'spam' THEN created_at END)";

fn stats_from_row(r: &rusqlite::Row, offset: usize) -> rusqlite::Result<ProjectStats> {
    Ok(ProjectStats {
        total: r.get(offset)?,
        open: r.get(offset + 1)?,
        new: r.get(offset + 2)?,
        bugs: r.get(offset + 3)?,
        feedback: r.get(offset + 4)?,
        spam: r.get(offset + 5)?,
        last_at: r.get(offset + 6)?,
    })
}

fn project_stats(c: &Connection, project_id: Option<i64>) -> rusqlite::Result<ProjectStats> {
    c.query_row(
        &format!("SELECT {STATS_SELECT} FROM feedback WHERE (?1 IS NULL OR project_id = ?1)"),
        params![project_id],
        |r| stats_from_row(r, 0),
    )
}

fn load_project(c: &Connection, id: i64) -> rusqlite::Result<Option<ProjectOut>> {
    let project = c
        .query_row(
            &format!("SELECT {} FROM projects WHERE id = ?1", Project::COLUMNS),
            params![id],
            Project::from_row,
        )
        .optional()?;
    match project {
        Some(project) => Ok(Some(ProjectOut {
            stats: project_stats(c, Some(project.id))?,
            project,
        })),
        None => Ok(None),
    }
}

async fn list_projects(
    _: Admin,
    State(state): State<AppState>,
) -> AppResult<Json<Vec<ProjectOut>>> {
    op_list_projects(&state).await.map(Json)
}

pub(crate) async fn op_list_projects(state: &AppState) -> AppResult<Vec<ProjectOut>> {
    let out = state
        .db
        .call(|c| {
            let mut stats: HashMap<i64, ProjectStats> = HashMap::new();
            let mut stmt = c.prepare(&format!(
                "SELECT project_id, {STATS_SELECT} FROM feedback GROUP BY project_id"
            ))?;
            for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, stats_from_row(r, 1)?)))? {
                let (id, s) = row?;
                stats.insert(id, s);
            }
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM projects ORDER BY title COLLATE NOCASE, id",
                Project::COLUMNS
            ))?;
            let projects = stmt
                .query_map([], Project::from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(projects
                .into_iter()
                .map(|project| ProjectOut {
                    stats: stats.get(&project.id).cloned().unwrap_or_default(),
                    project,
                })
                .collect())
        })
        .await?;
    Ok(out)
}

#[derive(Deserialize, Default)]
pub(crate) struct ProjectInput {
    pub(crate) title: Option<String>,
    pub(crate) slug: Option<String>,
    pub(crate) project_type: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) accepting: Option<bool>,
}

fn validate_title(t: &str) -> AppResult<String> {
    let t = t.trim();
    if t.is_empty() || t.chars().count() > MAX_PROJECT_TITLE {
        return Err(AppError::bad(
            "invalid_title",
            format!("title must be 1 to {MAX_PROJECT_TITLE} characters"),
        ));
    }
    Ok(t.to_string())
}

fn validate_slug(s: &str) -> AppResult<String> {
    let s = s.trim();
    if !valid_slug(s) {
        return Err(AppError::bad(
            "invalid_slug",
            "slug must be lowercase letters, digits and hyphens (max 63)",
        ));
    }
    Ok(s.to_string())
}

fn validate_type(t: &str) -> AppResult<String> {
    if !PROJECT_TYPES.contains(&t) {
        return Err(AppError::bad(
            "invalid_project_type",
            format!("project_type must be one of {}", PROJECT_TYPES.join(", ")),
        ));
    }
    Ok(t.to_string())
}

fn validate_description(d: &str) -> AppResult<String> {
    if d.chars().count() > MAX_DESCRIPTION {
        return Err(AppError::bad(
            "invalid_description",
            format!("description must be at most {MAX_DESCRIPTION} characters"),
        ));
    }
    Ok(d.trim().to_string())
}

async fn create_project(
    _: Admin,
    State(state): State<AppState>,
    JsonBody(input): JsonBody<ProjectInput>,
) -> AppResult<(StatusCode, Json<ProjectOut>)> {
    let out = op_create_project(&state, input).await?;
    Ok((StatusCode::CREATED, Json(out)))
}

pub(crate) async fn op_create_project(
    state: &AppState,
    input: ProjectInput,
) -> AppResult<ProjectOut> {
    let title = validate_title(input.title.as_deref().unwrap_or_default())?;
    let slug = match input
        .slug
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(s) => validate_slug(s)?,
        None => {
            let derived = slugify(&title);
            if derived.is_empty() {
                format!("project-{}", crypto::random_hex(3))
            } else {
                derived
            }
        }
    };
    let project_type = validate_type(input.project_type.as_deref().unwrap_or("other"))?;
    let description = validate_description(input.description.as_deref().unwrap_or_default())?;
    let accepting = input.accepting.unwrap_or(true);

    let out = state
        .db
        .call(move |c| {
            let ts = now();
            let res = c.execute(
                "INSERT INTO projects (slug, title, project_type, description, ingest_key, accepting, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![slug, title, project_type, description, new_ingest_key(), accepting, ts],
            );
            match res {
                Err(e) if is_unique_violation(&e) => Ok(Err(AppError::Conflict(format!("slug {slug:?} is already in use")))),
                Err(e) => Err(e.into()),
                Ok(_) => Ok(Ok(load_project(c, c.last_insert_rowid())?.expect("just inserted"))),
            }
        })
        .await??;
    Ok(out)
}

async fn get_project(
    _: Admin,
    State(state): State<AppState>,
    IdPath(id): IdPath,
) -> AppResult<Json<ProjectOut>> {
    op_get_project(&state, id).await.map(Json)
}

pub(crate) async fn op_get_project(state: &AppState, id: i64) -> AppResult<ProjectOut> {
    state
        .db
        .call(move |c| Ok(load_project(c, id)?))
        .await?
        .ok_or(AppError::NotFound)
}

async fn update_project(
    _: Admin,
    State(state): State<AppState>,
    IdPath(id): IdPath,
    JsonBody(input): JsonBody<ProjectInput>,
) -> AppResult<Json<ProjectOut>> {
    op_update_project(&state, id, input).await.map(Json)
}

pub(crate) async fn op_update_project(
    state: &AppState,
    id: i64,
    input: ProjectInput,
) -> AppResult<ProjectOut> {
    let title = input.title.as_deref().map(validate_title).transpose()?;
    let slug = input.slug.as_deref().map(validate_slug).transpose()?;
    let project_type = input
        .project_type
        .as_deref()
        .map(validate_type)
        .transpose()?;
    let description = input
        .description
        .as_deref()
        .map(validate_description)
        .transpose()?;
    let accepting = input.accepting;

    state
        .db
        .call(move |c| {
            let res = c.execute(
                "UPDATE projects SET title = COALESCE(?2, title), slug = COALESCE(?3, slug),
                    project_type = COALESCE(?4, project_type), description = COALESCE(?5, description),
                    accepting = COALESCE(?6, accepting), updated_at = ?7
                 WHERE id = ?1",
                params![id, title, slug, project_type, description, accepting, now()],
            );
            match res {
                Err(e) if is_unique_violation(&e) => Ok(Err(AppError::Conflict("slug is already in use".into()))),
                Err(e) => Err(e.into()),
                Ok(0) => Ok(Err(AppError::NotFound)),
                Ok(_) => Ok(Ok(load_project(c, id)?.expect("exists"))),
            }
        })
        .await?
}

async fn delete_project(
    _: Admin,
    State(state): State<AppState>,
    IdPath(id): IdPath,
) -> AppResult<StatusCode> {
    op_delete_project(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn op_delete_project(state: &AppState, id: i64) -> AppResult<()> {
    let deleted = state
        .db
        .call(move |c| Ok(c.execute("DELETE FROM projects WHERE id = ?1", params![id])?))
        .await?;
    if deleted == 0 {
        return Err(AppError::NotFound);
    }
    state.index.remove_project(id);
    Ok(())
}

async fn rotate_key(
    _: Admin,
    State(state): State<AppState>,
    IdPath(id): IdPath,
) -> AppResult<Json<ProjectOut>> {
    op_rotate_key(&state, id).await.map(Json)
}

pub(crate) async fn op_rotate_key(state: &AppState, id: i64) -> AppResult<ProjectOut> {
    state
        .db
        .call(move |c| {
            let n = c.execute(
                "UPDATE projects SET ingest_key = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, new_ingest_key(), now()],
            )?;
            if n == 0 {
                return Ok(None);
            }
            Ok(load_project(c, id)?)
        })
        .await?
        .ok_or(AppError::NotFound)
}

async fn export(
    _: Admin,
    State(state): State<AppState>,
    IdPath(id): IdPath,
    Query(q): Query<HashMap<String, String>>,
) -> AppResult<Response> {
    let format = q.get("format").map(String::as_str).unwrap_or("json");
    if format != "json" && format != "csv" {
        return Err(AppError::bad(
            "invalid_format",
            "format must be json or csv",
        ));
    }
    let (slug, items) = op_export_items(&state, id).await?;

    let (body, content_type) = if format == "csv" {
        (to_csv(&items), "text/csv; charset=utf-8")
    } else {
        (
            serde_json::to_string_pretty(&items).map_err(anyhow::Error::from)?,
            "application/json",
        )
    };
    let mut resp = body.into_response();
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{slug}-feedback.{format}\""
        ))
        .map_err(anyhow::Error::from)?,
    );
    Ok(resp)
}

/// The project slug and all its feedback, oldest first.
pub(crate) async fn op_export_items(
    state: &AppState,
    id: i64,
) -> AppResult<(String, Vec<Feedback>)> {
    state
        .db
        .call(move |c| {
            let Some(slug) = c
                .query_row(
                    "SELECT slug FROM projects WHERE id = ?1",
                    params![id],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
            else {
                return Ok(None);
            };
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM feedback WHERE project_id = ?1 ORDER BY created_at, id",
                Feedback::COLUMNS
            ))?;
            let items = stmt
                .query_map(params![id], Feedback::from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(Some((slug, items)))
        })
        .await?
        .ok_or(AppError::NotFound)
}

const CSV_HEADER: &[&str] = &[
    "id",
    "kind",
    "status",
    "title",
    "body",
    "contact",
    "rating",
    "meta",
    "source",
    "spam_reason",
    "dup_count",
    "admin_note",
    "created_at",
    "created_at_iso",
];

pub(crate) fn to_csv(items: &[Feedback]) -> String {
    let mut out = CSV_HEADER.join(",");
    out.push_str("\r\n");
    for f in items {
        let row = [
            f.id.to_string(),
            f.kind.clone(),
            f.status.clone(),
            f.title.clone(),
            f.body.clone(),
            f.contact.clone().unwrap_or_default(),
            f.rating.map(|r| r.to_string()).unwrap_or_default(),
            f.meta.to_string(),
            f.source.clone(),
            f.spam_reason.clone().unwrap_or_default(),
            f.dup_count.to_string(),
            f.admin_note.clone(),
            f.created_at.to_string(),
            iso(f.created_at),
        ];
        out.push_str(
            &row.iter()
                .map(|v| csv_field(v))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push_str("\r\n");
    }
    out
}

/// RFC 4180 quoting, plus a leading apostrophe on formula-like cells so
/// public input cannot run spreadsheet formulas when the export is opened.
pub fn csv_field(v: &str) -> String {
    let v = if v.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{v}")
    } else {
        v.to_string()
    };
    if v.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v
    }
}

fn iso(ts: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp(ts)
        .ok()
        .and_then(|t| {
            t.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Feedback

#[derive(Clone, Copy)]
enum StatusFilter {
    Open,
    All,
    Any,
    Exact(&'static str),
}

struct Filters {
    project_id: Option<i64>,
    kind: Option<&'static str>,
    status: StatusFilter,
    from: Option<i64>,
    to: Option<i64>,
}

impl Filters {
    fn sql(&self) -> (String, Vec<SqlValue>) {
        let mut clauses = vec!["1 = 1".to_string()];
        let mut args = Vec::new();
        if let Some(p) = self.project_id {
            clauses.push("project_id = ?".into());
            args.push(SqlValue::Integer(p));
        }
        if let Some(k) = self.kind {
            clauses.push("kind = ?".into());
            args.push(SqlValue::Text(k.into()));
        }
        match self.status {
            StatusFilter::Open => clauses.push("status IN ('new', 'triaged')".into()),
            StatusFilter::All => clauses.push("status != 'spam'".into()),
            StatusFilter::Any => {}
            StatusFilter::Exact(s) => {
                clauses.push("status = ?".into());
                args.push(SqlValue::Text(s.into()));
            }
        }
        if let Some(f) = self.from {
            clauses.push("created_at >= ?".into());
            args.push(SqlValue::Integer(f));
        }
        if let Some(t) = self.to {
            clauses.push("created_at <= ?".into());
            args.push(SqlValue::Integer(t));
        }
        (clauses.join(" AND "), args)
    }
}

fn parse_int(q: &HashMap<String, String>, key: &str) -> AppResult<Option<i64>> {
    match q.get(key).map(|v| v.trim()).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) => v
            .parse()
            .map(Some)
            .map_err(|_| AppError::bad("invalid_query", format!("{key} must be an integer"))),
    }
}

fn parse_filters(q: &HashMap<String, String>) -> AppResult<Filters> {
    let kind = match q.get("kind").map(|v| v.trim()).filter(|v| !v.is_empty()) {
        None => None,
        Some(k) => Some(
            *KINDS
                .iter()
                .find(|x| **x == k)
                .ok_or_else(|| AppError::bad("invalid_query", "kind must be bug or feedback"))?,
        ),
    };
    let status = match q.get("status").map(|v| v.trim()).filter(|v| !v.is_empty()) {
        None | Some("open") => StatusFilter::Open,
        Some("all") => StatusFilter::All,
        Some("any") => StatusFilter::Any,
        Some(s) => StatusFilter::Exact(
            STATUSES
                .iter()
                .find(|x| **x == s)
                .ok_or_else(|| AppError::bad("invalid_query", "unknown status filter"))?,
        ),
    };
    Ok(Filters {
        project_id: parse_int(q, "project_id")?,
        kind,
        status,
        from: parse_int(q, "from")?,
        to: parse_int(q, "to")?,
    })
}

fn parse_mode(q: &HashMap<String, String>) -> AppResult<Mode> {
    match q.get("mode").map(|v| v.trim()).filter(|v| !v.is_empty()) {
        None | Some("hybrid") => Ok(Mode::Hybrid),
        Some("fuzzy") => Ok(Mode::Fuzzy),
        Some("keyword") => Ok(Mode::Keyword),
        Some("semantic") => Ok(Mode::Semantic),
        Some(_) => Err(AppError::bad(
            "invalid_query",
            "mode must be hybrid, fuzzy, keyword or semantic",
        )),
    }
}

fn semantic_floor(e: &Embedder) -> f32 {
    match e {
        Embedder::Hash { .. } => 0.15,
        _ => 0.3,
    }
}

/// Embeds the query and returns semantic candidates, or `None` when vectors are unavailable.
pub(crate) async fn semantic_hits(
    state: &AppState,
    q: &str,
    project: Option<i64>,
) -> Option<Vec<(i64, f64)>> {
    if !state.embedder.enabled() {
        return None;
    }
    match state.embedder.embed(&[q.to_string()]).await {
        Ok(mut v) if !v.is_empty() => {
            let qv = v.swap_remove(0);
            let hits = state.index.search(&qv, project, 200, None);
            Some(search::cut_semantic(hits, semantic_floor(&state.embedder)))
        }
        Ok(_) => None,
        Err(e) => {
            tracing::warn!(error = %e, "query embedding failed, falling back to lexical search");
            None
        }
    }
}

/// Ranked `(id, score)` candidates for a query, the effective mode and whether vectors were used.
async fn ranked(
    state: &AppState,
    q: &str,
    mode: Mode,
    filters: &Filters,
) -> AppResult<(Vec<(i64, f64)>, Mode, bool)> {
    let project = filters.project_id;
    let (scope_sql, scope_args) = filters.sql();
    let semantic = if matches!(mode, Mode::Hybrid | Mode::Semantic) {
        semantic_hits(state, q, project).await
    } else {
        None
    };
    let used_vectors = semantic.is_some();
    let effective = if mode == Mode::Semantic && semantic.is_none() {
        Mode::Hybrid
    } else {
        mode
    };
    let query = q.to_string();
    let (keyword, fuzzy) = state
        .db
        .call(move |c| {
            let scope = search::Scope {
                sql: &scope_sql,
                args: &scope_args,
            };
            let kw = if matches!(effective, Mode::Hybrid | Mode::Keyword) {
                search::keyword(c, &query, scope)?
            } else {
                vec![]
            };
            let fz = if matches!(effective, Mode::Hybrid | Mode::Fuzzy) {
                search::fuzzy(c, &query, scope)?
            } else {
                vec![]
            };
            Ok((kw, fz))
        })
        .await?;
    let list = match effective {
        Mode::Keyword => keyword,
        Mode::Fuzzy => fuzzy,
        Mode::Semantic => semantic.unwrap_or_default(),
        Mode::Hybrid => {
            let mut lists = vec![keyword, fuzzy];
            if let Some(s) = semantic {
                lists.push(s);
            }
            search::rrf(&lists)
        }
    };
    Ok((list, effective, used_vectors))
}

/// Loads rows for `ids` that pass `filters`, preserving the order of `ids`.
fn fetch_ranked(
    c: &Connection,
    ranked: &[(i64, f64)],
    filters: &Filters,
) -> rusqlite::Result<Vec<Feedback>> {
    if ranked.is_empty() {
        return Ok(vec![]);
    }
    let (where_sql, mut args) = filters.sql();
    let placeholders = vec!["?"; ranked.len()].join(",");
    args.extend(ranked.iter().map(|(id, _)| SqlValue::Integer(*id)));
    let mut stmt = c.prepare(&format!(
        "SELECT {} FROM feedback WHERE {where_sql} AND id IN ({placeholders})",
        Feedback::COLUMNS
    ))?;
    let mut rows: HashMap<i64, Feedback> = stmt
        .query_map(params_from_iter(args), Feedback::from_row)?
        .map(|r| r.map(|f| (f.id, f)))
        .collect::<rusqlite::Result<_>>()?;
    Ok(ranked
        .iter()
        .filter_map(|(id, score)| {
            rows.remove(id).map(|mut f| {
                f.score = Some(*score);
                f
            })
        })
        .collect())
}

async fn list_feedback(
    _: Admin,
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> AppResult<Json<Value>> {
    op_list_feedback(&state, &q).await.map(Json)
}

/// Lists or searches feedback. `q` uses the same keys as the HTTP query string.
pub(crate) async fn op_list_feedback(
    state: &AppState,
    q: &HashMap<String, String>,
) -> AppResult<Value> {
    let filters = parse_filters(q)?;
    let mode = parse_mode(q)?;
    let limit = parse_int(q, "limit")?.unwrap_or(50).clamp(1, 200);
    let offset = parse_int(q, "offset")?.unwrap_or(0).max(0);
    let text = q
        .get("q")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let Some(text) = text else {
        let (items, total) = state
            .db
            .call(move |c| {
                let (where_sql, args) = filters.sql();
                let total: i64 = c.query_row(
                    &format!("SELECT COUNT(*) FROM feedback WHERE {where_sql}"),
                    params_from_iter(args.clone()),
                    |r| r.get(0),
                )?;
                let mut page_args = args;
                page_args.push(SqlValue::Integer(limit));
                page_args.push(SqlValue::Integer(offset));
                let mut stmt = c.prepare(&format!(
                    "SELECT {} FROM feedback WHERE {where_sql} ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?",
                    Feedback::COLUMNS
                ))?;
                let items = stmt
                    .query_map(params_from_iter(page_args), Feedback::from_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok((items, total))
            })
            .await?;
        return Ok(json!({ "items": items, "total": total, "mode": mode, "semantic": false }));
    };

    let (list, effective, used_vectors) = ranked(state, &text, mode, &filters).await?;
    let all = state
        .db
        .call(move |c| Ok(fetch_ranked(c, &list, &filters)?))
        .await?;
    let total = all.len();
    let items: Vec<Feedback> = all
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect();
    Ok(json!({ "items": items, "total": total, "mode": effective, "semantic": used_vectors }))
}

fn load_feedback(c: &Connection, id: i64) -> rusqlite::Result<Option<Feedback>> {
    c.query_row(
        &format!("SELECT {} FROM feedback WHERE id = ?1", Feedback::COLUMNS),
        params![id],
        Feedback::from_row,
    )
    .optional()
}

async fn get_feedback(
    _: Admin,
    State(state): State<AppState>,
    IdPath(id): IdPath,
) -> AppResult<Json<Value>> {
    op_get_feedback(&state, id).await.map(Json)
}

/// One feedback item plus its nearest neighbours in the same project.
pub(crate) async fn op_get_feedback(state: &AppState, id: i64) -> AppResult<Value> {
    let item = state
        .db
        .call(move |c| Ok(load_feedback(c, id)?))
        .await?
        .ok_or(AppError::NotFound)?;
    let neighbours = state
        .index
        .get(id)
        .map(|v| {
            state
                .index
                .search(&v, Some(item.project_id), SIMILAR * 3, Some(id))
        })
        .unwrap_or_default();
    let ranked: Vec<(i64, f64)> = neighbours.into_iter().map(|(i, s)| (i, s as f64)).collect();
    let filters = Filters {
        project_id: Some(item.project_id),
        kind: None,
        status: StatusFilter::All,
        from: None,
        to: None,
    };
    let mut similar = state
        .db
        .call(move |c| Ok(fetch_ranked(c, &ranked, &filters)?))
        .await?;
    similar.truncate(SIMILAR);
    Ok(json!({ "item": item, "similar": similar }))
}

/// Feedback similar to `vector` (cosine), excluding `exclude`, honouring the usual filters.
pub(crate) async fn op_similar_to_vector(
    state: &AppState,
    vector: &[f32],
    project_id: Option<i64>,
    exclude: Option<i64>,
    limit: usize,
) -> AppResult<Vec<Feedback>> {
    let hits = state.index.search(vector, project_id, limit * 3, exclude);
    let ranked: Vec<(i64, f64)> = hits.into_iter().map(|(i, s)| (i, s as f64)).collect();
    let filters = Filters {
        project_id,
        kind: None,
        status: StatusFilter::All,
        from: None,
        to: None,
    };
    let mut items = state
        .db
        .call(move |c| Ok(fetch_ranked(c, &ranked, &filters)?))
        .await?;
    items.truncate(limit);
    Ok(items)
}

#[derive(Deserialize, Default)]
pub(crate) struct FeedbackPatch {
    pub(crate) status: Option<String>,
    pub(crate) admin_note: Option<String>,
    pub(crate) kind: Option<String>,
}

pub(crate) fn validate_status(s: &str) -> AppResult<String> {
    if !STATUSES.contains(&s) {
        return Err(AppError::bad(
            "invalid_status",
            format!("status must be one of {}", STATUSES.join(", ")),
        ));
    }
    Ok(s.to_string())
}

async fn update_feedback(
    _: Admin,
    State(state): State<AppState>,
    IdPath(id): IdPath,
    JsonBody(patch): JsonBody<FeedbackPatch>,
) -> AppResult<Json<Feedback>> {
    op_update_feedback(&state, id, patch).await.map(Json)
}

pub(crate) async fn op_update_feedback(
    state: &AppState,
    id: i64,
    patch: FeedbackPatch,
) -> AppResult<Feedback> {
    let status = patch.status.as_deref().map(validate_status).transpose()?;
    let kind = match patch.kind.as_deref() {
        Some(k) if !KINDS.contains(&k) => {
            return Err(AppError::bad(
                "invalid_kind",
                "kind must be bug or feedback",
            ));
        }
        k => k.map(str::to_string),
    };
    if patch
        .admin_note
        .as_ref()
        .is_some_and(|n| n.chars().count() > MAX_NOTE)
    {
        return Err(AppError::bad(
            "invalid_admin_note",
            format!("admin_note must be at most {MAX_NOTE} characters"),
        ));
    }
    let note = patch.admin_note;
    state
        .db
        .call(move |c| {
            // Leaving the spam bucket clears the quarantine reason.
            let n = c.execute(
                "UPDATE feedback SET status = COALESCE(?2, status), admin_note = COALESCE(?3, admin_note),
                    kind = COALESCE(?4, kind),
                    spam_reason = CASE WHEN COALESCE(?2, status) = 'spam' THEN spam_reason ELSE NULL END,
                    updated_at = ?5
                 WHERE id = ?1",
                params![id, status, note, kind, now()],
            )?;
            if n == 0 {
                return Ok(None);
            }
            Ok(load_feedback(c, id)?)
        })
        .await?
        .ok_or(AppError::NotFound)
}

async fn delete_feedback(
    _: Admin,
    State(state): State<AppState>,
    IdPath(id): IdPath,
) -> AppResult<StatusCode> {
    op_delete_feedback(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn op_delete_feedback(state: &AppState, id: i64) -> AppResult<()> {
    let n = state
        .db
        .call(move |c| Ok(c.execute("DELETE FROM feedback WHERE id = ?1", params![id])?))
        .await?;
    if n == 0 {
        return Err(AppError::NotFound);
    }
    state.index.remove(id);
    Ok(())
}

#[derive(Deserialize)]
pub(crate) struct BulkBody {
    pub(crate) ids: Vec<i64>,
    pub(crate) action: String,
    pub(crate) status: Option<String>,
}

async fn bulk(
    _: Admin,
    State(state): State<AppState>,
    JsonBody(body): JsonBody<BulkBody>,
) -> AppResult<Json<Value>> {
    op_bulk(&state, body).await.map(Json)
}

pub(crate) async fn op_bulk(state: &AppState, body: BulkBody) -> AppResult<Value> {
    if body.ids.is_empty() || body.ids.len() > MAX_BULK {
        return Err(AppError::bad(
            "invalid_ids",
            format!("ids must contain 1 to {MAX_BULK} entries"),
        ));
    }
    let status = match body.action.as_str() {
        "set_status" => Some(validate_status(body.status.as_deref().unwrap_or_default())?),
        "delete" => None,
        _ => {
            return Err(AppError::bad(
                "invalid_action",
                "action must be set_status or delete",
            ));
        }
    };
    let ids = body.ids.clone();
    let affected = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let mut affected = 0;
            for id in &ids {
                affected += match &status {
                    Some(s) => tx.execute(
                        "UPDATE feedback SET status = ?2,
                            spam_reason = CASE WHEN ?2 = 'spam' THEN spam_reason ELSE NULL END, updated_at = ?3
                         WHERE id = ?1",
                        params![id, s, now()],
                    )?,
                    None => tx.execute("DELETE FROM feedback WHERE id = ?1", params![id])?,
                };
            }
            tx.commit()?;
            Ok(affected)
        })
        .await?;
    if body.action == "delete" {
        for id in &body.ids {
            state.index.remove(*id);
        }
    }
    Ok(json!({ "affected": affected }))
}

// ---------------------------------------------------------------------------
// Stats and maintenance

const BLOCK_REASONS: &[&str] = &[
    "rate_limited",
    "honeypot",
    "pow_insufficient",
    "duplicate",
    "spam_score",
    "too_fast",
    "near_duplicate",
];

async fn stats(
    _: Admin,
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> AppResult<Json<Value>> {
    let project_id = parse_int(&q, "project_id")?;
    op_stats(&state, project_id).await.map(Json)
}

pub(crate) async fn op_stats(state: &AppState, project_id: Option<i64>) -> AppResult<Value> {
    let model = state.embedder.model_id();
    let enabled = state.embedder.enabled();
    let out = state
        .db
        .call(move |c| {
            let ts = now();
            let s = project_stats(c, project_id)?;
            let last_7d: i64 = c.query_row(
                "SELECT COUNT(*) FROM feedback WHERE status != 'spam' AND created_at > ?2
                 AND (?1 IS NULL OR project_id = ?1)",
                params![project_id, ts - 7 * 86400],
                |r| r.get(0),
            )?;

            let today = time::OffsetDateTime::from_unix_timestamp(ts)?.date();
            let start = today - time::Duration::days(29);
            let start_ts = start.midnight().assume_utc().unix_timestamp();
            let mut counts: HashMap<(String, String), i64> = HashMap::new();
            let mut stmt = c.prepare(
                "SELECT date(created_at, 'unixepoch'), kind, COUNT(*) FROM feedback
                 WHERE status != 'spam' AND created_at >= ?2 AND (?1 IS NULL OR project_id = ?1)
                 GROUP BY 1, 2",
            )?;
            for row in stmt.query_map(params![project_id, start_ts], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))? {
                let (day, kind, n): (String, String, i64) = row?;
                counts.insert((day, kind), n);
            }
            let mut daily = Vec::with_capacity(30);
            let mut day = start;
            while day <= today {
                let key = format!("{:04}-{:02}-{:02}", day.year(), day.month() as u8, day.day());
                daily.push(json!({
                    "bug": counts.get(&(key.clone(), "bug".to_string())).copied().unwrap_or(0),
                    "feedback": counts.get(&(key.clone(), "feedback".to_string())).copied().unwrap_or(0),
                    "day": key,
                }));
                day = day.next_day().expect("date in range");
            }

            let mut blocked: serde_json::Map<String, Value> =
                BLOCK_REASONS.iter().map(|r| (r.to_string(), json!(0))).collect();
            let mut stmt = c.prepare(
                "SELECT reason, SUM(n) FROM block_counts WHERE day > ?2 AND (?1 IS NULL OR project_id = ?1)
                 GROUP BY reason",
            )?;
            for row in stmt.query_map(params![project_id, ts / 86400 - 30], |r| Ok((r.get(0)?, r.get(1)?)))? {
                let (reason, n): (String, i64) = row?;
                blocked.insert(reason, json!(n));
            }

            let (all, embedded): (i64, i64) = c.query_row(
                "SELECT COUNT(*), COUNT(e.feedback_id) FROM feedback f
                 LEFT JOIN embeddings e ON e.feedback_id = f.id AND e.model = ?2
                 WHERE (?1 IS NULL OR f.project_id = ?1)",
                params![project_id, model],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;

            Ok(json!({
                "totals": {
                    "total": s.total, "open": s.open, "new": s.new, "bugs": s.bugs,
                    "feedback": s.feedback, "spam": s.spam, "last_7d": last_7d,
                },
                "daily": daily,
                "blocked": blocked,
                "embeddings": {
                    "enabled": enabled,
                    "model": model,
                    "embedded": if enabled { embedded } else { 0 },
                    "pending": if enabled { all - embedded } else { 0 },
                },
            }))
        })
        .await?;
    Ok(out)
}

async fn reindex(_: Admin, State(state): State<AppState>) -> AppResult<(StatusCode, Json<Value>)> {
    op_reindex(&state).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "ok": true }))))
}

/// Drops every vector and wakes the worker to re-embed everything.
pub(crate) async fn op_reindex(state: &AppState) -> AppResult<()> {
    state
        .db
        .call(|c| Ok(c.execute("DELETE FROM embeddings", [])?))
        .await?;
    state.index.clear();
    state.embed_notify.notify_one();
    Ok(())
}

// ---------------------------------------------------------------------------
// API tokens

async fn list_tokens(
    _: Admin,
    State(state): State<AppState>,
) -> AppResult<Json<Vec<tokens::Token>>> {
    let list = state.db.call(|c| Ok(tokens::list(c)?)).await?;
    Ok(Json(list))
}

#[derive(Deserialize)]
struct TokenInput {
    name: Option<String>,
    scope: Option<String>,
}

async fn create_token(
    _: Admin,
    State(state): State<AppState>,
    JsonBody(input): JsonBody<TokenInput>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let name = tokens::validate_name(input.name.as_deref().unwrap_or_default())?;
    let scope = Scope::parse(input.scope.as_deref().unwrap_or_default().trim())
        .ok_or_else(|| AppError::bad("invalid_scope", "scope must be read or write"))?;
    let (raw, token) = state
        .db
        .call(move |c| Ok(tokens::create(c, &name, scope)?))
        .await?;
    let mut out = serde_json::to_value(&token).map_err(anyhow::Error::from)?;
    out["token"] = json!(raw);
    Ok((StatusCode::CREATED, Json(out)))
}

async fn delete_token(
    _: Admin,
    State(state): State<AppState>,
    IdPath(id): IdPath,
) -> AppResult<StatusCode> {
    let deleted = state.db.call(move |c| Ok(tokens::delete(c, id)?)).await?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::csv_field;

    #[test]
    fn csv_quoting_and_formula_guard() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("line\nbreak"), "\"line\nbreak\"");
        assert_eq!(csv_field("=HYPERLINK(1)"), "'=HYPERLINK(1)");
        assert_eq!(csv_field("-1,2"), "\"'-1,2\"");
    }
}
