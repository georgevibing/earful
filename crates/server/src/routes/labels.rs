//! Project-scoped labels. HTTP and MCP share every validation and mutation here.
use std::collections::{BTreeSet, HashMap};

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{IdPath, JsonBody};
use crate::{
    auth::Admin,
    db::now,
    error::{AppError, AppResult},
    models::{Feedback, Label},
    state::AppState,
};

pub(crate) const COLORS: &[&str] = &["slate", "blue", "green", "amber", "red", "violet", "pink"];
pub(crate) const MAX_LABELS: usize = 20;

#[derive(Serialize)]
pub(crate) struct LabelOut {
    #[serde(flatten)]
    pub label: Label,
    pub feedback_count: i64,
    pub open_count: i64,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct LabelInput {
    pub project_id: Option<i64>,
    pub name: Option<String>,
    pub color: Option<String>,
    pub description: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AssignInput {
    pub ids: Vec<i64>,
    pub label_ids: Vec<i64>,
    pub action: String,
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::bad("invalid_labels", message)
}

fn validate(input: &LabelInput) -> AppResult<()> {
    if let Some(name) = &input.name
        && (name.trim().is_empty()
            || name.trim().chars().count() > 64
            || name.chars().any(char::is_control))
    {
        return Err(invalid(
            "name must be 1..64 characters without control characters",
        ));
    }
    if input.color.as_deref().is_some_and(|c| !COLORS.contains(&c)) {
        return Err(invalid(format!(
            "color must be one of {}",
            COLORS.join(", ")
        )));
    }
    if input
        .description
        .as_ref()
        .is_some_and(|s| s.chars().count() > 500)
    {
        return Err(invalid("description must be at most 500 characters"));
    }
    Ok(())
}

fn project_exists(c: &Connection, id: i64) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)",
        [id],
        |r| r.get(0),
    )
}

fn load(c: &Connection, project: Option<i64>, id: Option<i64>) -> rusqlite::Result<Vec<LabelOut>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {}, COUNT(CASE WHEN f.status != 'spam' THEN 1 END),
         COUNT(CASE WHEN f.status IN ('new', 'triaged') THEN 1 END)
         FROM labels l LEFT JOIN feedback_labels fl ON fl.label_id = l.id
         LEFT JOIN feedback f ON f.id = fl.feedback_id
         WHERE (?1 IS NULL OR l.project_id = ?1) AND (?2 IS NULL OR l.id = ?2)
         GROUP BY l.id ORDER BY 9 DESC, l.name_key, l.id",
        Label::COLUMNS
    ))?;
    stmt.query_map(params![project, id], |r| {
        Ok(LabelOut {
            label: Label::from_row(r, 0)?,
            feedback_count: r.get(7)?,
            open_count: r.get(8)?,
        })
    })?
    .collect()
}

pub(super) async fn list(
    _: Admin,
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> AppResult<Json<Vec<LabelOut>>> {
    let project = q
        .get("project_id")
        .map(|s| {
            s.parse::<i64>()
                .map_err(|_| AppError::bad("invalid_query", "project_id must be an integer"))
        })
        .transpose()?;
    op_list_labels(&state, project).await.map(Json)
}

pub(crate) async fn op_list_labels(
    state: &AppState,
    project: Option<i64>,
) -> AppResult<Vec<LabelOut>> {
    state
        .db
        .call(move |c| {
            if let Some(id) = project
                && !project_exists(c, id)?
            {
                return Ok(Err(AppError::NotFound));
            }
            Ok(Ok(load(c, project, None)?))
        })
        .await?
}

pub(super) async fn create(
    _: Admin,
    State(state): State<AppState>,
    JsonBody(input): JsonBody<LabelInput>,
) -> AppResult<(StatusCode, Json<LabelOut>)> {
    Ok((
        StatusCode::CREATED,
        Json(op_create_label(&state, input).await?),
    ))
}

pub(crate) async fn op_create_label(state: &AppState, input: LabelInput) -> AppResult<LabelOut> {
    validate(&input)?;
    let project = input
        .project_id
        .filter(|p| *p > 0)
        .ok_or_else(|| invalid("project_id is required"))?;
    let name = input
        .name
        .as_deref()
        .ok_or_else(|| invalid("name is required"))?
        .trim()
        .to_string();
    state.db.call(move |c| {
        let tx = c.transaction()?;
        if !project_exists(&tx, project)? { return Ok(Err(AppError::NotFound)); }
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM labels WHERE project_id = ?1", [project], |r| r.get(0))?;
        if count >= 200 { return Ok(Err(invalid("a project can have at most 200 labels"))); }
        let ts = now();
        let result = tx.execute("INSERT INTO labels(project_id, name, name_key, color, description, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![project, name, name.to_lowercase(), input.color.unwrap_or_else(|| "slate".into()), input.description.unwrap_or_default(), ts]);
        if let Err(e) = result {
            if e.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                return Ok(Err(AppError::Conflict("a label with that name already exists in this project".into())));
            }
            return Err(e.into());
        }
        let label = load(&tx, None, Some(tx.last_insert_rowid()))?.remove(0);
        tx.commit()?;
        Ok(Ok(label))
    }).await?
}

pub(super) async fn update(
    _: Admin,
    State(state): State<AppState>,
    IdPath(id): IdPath,
    JsonBody(input): JsonBody<LabelInput>,
) -> AppResult<Json<LabelOut>> {
    op_update_label(&state, id, input).await.map(Json)
}

pub(crate) async fn op_update_label(
    state: &AppState,
    id: i64,
    input: LabelInput,
) -> AppResult<LabelOut> {
    validate(&input)?;
    if input.project_id.is_some() {
        return Err(invalid("labels cannot move between projects"));
    }
    if input.name.is_none() && input.color.is_none() && input.description.is_none() {
        return Err(invalid("nothing to update"));
    }
    let name = input.name.map(|s| s.trim().to_string());
    state.db.call(move |c| {
        let result = c.execute("UPDATE labels SET name = COALESCE(?2, name), name_key = COALESCE(?3, name_key), color = COALESCE(?4, color), description = COALESCE(?5, description), updated_at = ?6 WHERE id = ?1",
            params![id, name, name.as_ref().map(|s| s.to_lowercase()), input.color, input.description, now()]);
        match result {
            Ok(0) => return Ok(Err(AppError::NotFound)),
            Err(e) if e.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) => return Ok(Err(AppError::Conflict("a label with that name already exists in this project".into()))),
            Err(e) => return Err(e.into()),
            _ => {}
        }
        Ok(Ok(load(c, None, Some(id))?.remove(0)))
    }).await?
}

pub(super) async fn remove(
    _: Admin,
    State(state): State<AppState>,
    IdPath(id): IdPath,
) -> AppResult<StatusCode> {
    op_delete_label(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn op_delete_label(state: &AppState, id: i64) -> AppResult<()> {
    state.db.call(move |c| {
        let tx = c.transaction()?;
        tx.execute("UPDATE feedback SET updated_at = ?2 WHERE id IN (SELECT feedback_id FROM feedback_labels WHERE label_id = ?1)", params![id, now()])?;
        let n = tx.execute("DELETE FROM labels WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(if n == 0 { Err(AppError::NotFound) } else { Ok(()) })
    }).await?
}

pub(super) async fn assign(
    _: Admin,
    State(state): State<AppState>,
    JsonBody(input): JsonBody<AssignInput>,
) -> AppResult<Json<Value>> {
    op_assign_labels(&state, input).await.map(Json)
}

pub(crate) async fn op_assign_labels(state: &AppState, input: AssignInput) -> AppResult<Value> {
    if input.ids.is_empty() || input.ids.len() > 500 || input.ids.iter().any(|i| *i <= 0) {
        return Err(invalid("ids must contain 1..500 positive feedback ids"));
    }
    if input.label_ids.len() > MAX_LABELS || input.label_ids.iter().any(|i| *i <= 0) {
        return Err(invalid(
            "label_ids must contain at most 20 positive label ids",
        ));
    }
    if !["add", "remove", "set"].contains(&input.action.as_str())
        || (input.label_ids.is_empty() && input.action != "set")
    {
        return Err(invalid(
            "action must be add, remove, or set; only set accepts empty label_ids",
        ));
    }
    let ids: BTreeSet<_> = input.ids.into_iter().collect();
    let label_ids: BTreeSet<_> = input.label_ids.into_iter().collect();
    state.db.call(move |c| {
        let tx = c.transaction()?;
        let mut projects = BTreeSet::new();
        for lid in &label_ids {
            let Some(project) = tx.query_row("SELECT project_id FROM labels WHERE id = ?1", [lid], |r| r.get::<_, i64>(0)).optional()? else { return Ok(Err(AppError::NotFound)); };
            projects.insert(project);
        }
        let mut affected = 0;
        let ts = now();
        for fid in ids {
            let Some(project) = tx.query_row("SELECT project_id FROM feedback WHERE id = ?1", [fid], |r| r.get::<_, i64>(0)).optional()? else { return Ok(Err(AppError::NotFound)); };
            if projects.iter().any(|p| *p != project) { return Ok(Err(invalid("every label must belong to the feedback item's project"))); }
            let current: BTreeSet<i64> = tx.prepare("SELECT label_id FROM feedback_labels WHERE feedback_id = ?1")?.query_map([fid], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            let next: BTreeSet<_> = match input.action.as_str() {
                "add" => current.union(&label_ids).copied().collect(),
                "remove" => current.difference(&label_ids).copied().collect(),
                _ => label_ids.clone(),
            };
            if next.len() > MAX_LABELS { return Ok(Err(invalid("feedback can have at most 20 labels"))); }
            if next == current { continue; }
            // Delta updates avoid touching unrelated assignments.
            for lid in current.difference(&next) { tx.execute("DELETE FROM feedback_labels WHERE feedback_id = ?1 AND label_id = ?2", params![fid, lid])?; }
            for lid in next.difference(&current) { tx.execute("INSERT INTO feedback_labels(feedback_id, label_id, project_id) VALUES (?1, ?2, ?3)", params![fid, lid, project])?; }
            tx.execute("UPDATE feedback SET updated_at = ?2 WHERE id = ?1", params![fid, ts])?;
            affected += 1;
        }
        tx.commit()?;
        Ok(Ok(json!({"affected": affected})))
    }).await?
}

/// Batch hydration for pages, exports and neighbours, bounded below SQLite bind limits.
pub(crate) fn attach(c: &Connection, items: &mut [Feedback]) -> rusqlite::Result<()> {
    for batch in items.chunks_mut(500) {
        attach_batch(c, batch)?;
    }
    Ok(())
}

fn attach_batch(c: &Connection, items: &mut [Feedback]) -> rusqlite::Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let placeholders = vec!["?"; items.len()].join(",");
    let mut stmt = c.prepare(&format!("SELECT fl.feedback_id, {} FROM feedback_labels fl JOIN labels l ON l.id = fl.label_id WHERE fl.feedback_id IN ({placeholders}) ORDER BY l.name_key, l.id", Label::COLUMNS))?;
    let mut labels: HashMap<i64, Vec<Label>> = HashMap::new();
    for row in stmt.query_map(params_from_iter(items.iter().map(|f| f.id)), |r| {
        Ok((r.get::<_, i64>(0)?, Label::from_row(r, 1)?))
    })? {
        let (fid, label) = row?;
        labels.entry(fid).or_default().push(label);
    }
    for item in items {
        item.labels = labels.remove(&item.id).unwrap_or_default();
    }
    Ok(())
}
