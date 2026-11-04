//! Anonymous ingestion: challenge issuing, submission pipeline and the hosted page.

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ClientIp, JsonBody, html_escape};
use crate::{
    db::{now, now_ms},
    error::{AppError, AppResult},
    pow,
    ratelimit::{self, Rule},
    spam,
    state::AppState,
};

const MAX_TITLE: usize = 200;
const MAX_BODY: usize = 10_000;
const MIN_BODY: usize = 3;
const MAX_CONTACT: usize = 200;
const MAX_META_BYTES: usize = 4096;
const DERIVED_TITLE: usize = 80;

fn ingest_key(headers: &HeaderMap, from_body: Option<&str>) -> Option<String> {
    from_body
        .map(str::to_string)
        .or_else(|| {
            headers
                .get("x-fh-key")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        })
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty() && k.len() <= 100)
}

/// Looks up `(project_id, accepting)` by ingest key.
fn project_by_key(conn: &Connection, key: &str) -> rusqlite::Result<Option<(i64, bool)>> {
    conn.query_row(
        "SELECT id, accepting FROM projects WHERE ingest_key = ?1",
        params![key],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

fn paused() -> AppError {
    AppError::Forbidden(
        "project_paused",
        "this project is not accepting feedback right now".into(),
    )
}

pub async fn challenge(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> AppResult<Response> {
    let key = ingest_key(&headers, q.get("key").map(String::as_str)).ok_or(AppError::NotFound)?;
    let cfg = state.cfg.clone();
    let secret = state.secret.clone();
    let issued = state
        .db
        .call(move |c| {
            let Some((pid, accepting)) = project_by_key(c, &key)? else {
                return Ok(Err(AppError::NotFound));
            };
            if !accepting {
                return Ok(Err(paused()));
            }
            let ts = now();
            let rule = [Rule {
                bucket: "challenge",
                ip_hash: Some(&ip),
                window_secs: 3600,
                limit: cfg.limits.challenges_per_ip_hour,
            }];
            if let Err(retry_after) = ratelimit::check(c, ts, &rule)? {
                ratelimit::record_block(c, Some(pid), "rate_limited", ts)?;
                return Ok(Err(AppError::RateLimited { retry_after }));
            }
            ratelimit::record(c, "challenge", &ip, ts)?;

            // Adaptive difficulty: repeat senders and busy projects pay more work.
            let bucket = format!("submit:{pid}");
            let recent = ratelimit::count(c, &bucket, Some(&ip), ts - 3600)?;
            let load = ratelimit::count(c, &bucket, None, ts - 3600)?;
            let mut bits = cfg.pow.base_bits + recent.min(4);
            if load * 2 > cfg.limits.per_project_hour {
                bits += 2;
            }
            let bits = bits.min(cfg.pow.max_bits.max(cfg.pow.base_bits));
            Ok(Ok((pow::issue(&secret, pid, now_ms(), bits), bits)))
        })
        .await??;

    let (token, bits) = issued;
    let mut resp = Json(json!({
        "challenge": token,
        "bits": bits,
        "expires_in": state.cfg.pow.challenge_ttl_secs,
    }))
    .into_response();
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(resp)
}

#[derive(Deserialize)]
pub struct SubmitBody {
    key: Option<String>,
    kind: Option<String>,
    title: Option<String>,
    body: Option<String>,
    contact: Option<String>,
    rating: Option<i64>,
    meta: Option<Value>,
    diagnostics: Option<Value>,
    source: Option<String>,
    challenge: Option<String>,
    nonce: Option<Value>,
    website: Option<String>,
}

struct Clean {
    kind: String,
    title: String,
    body: String,
    contact: Option<String>,
    rating: Option<i64>,
    meta: String,
    diagnostics: String,
    source: String,
}

fn validate(b: &SubmitBody) -> AppResult<Clean> {
    let kind = b.kind.as_deref().map(str::trim).unwrap_or_default();
    if !crate::models::KINDS.contains(&kind) {
        return Err(AppError::bad(
            "invalid_kind",
            "kind must be bug, feedback, crash, exception or log",
        ));
    }
    let body = spam::sanitize(b.body.as_deref().unwrap_or_default());
    let body_len = body.chars().count();
    if !(MIN_BODY..=MAX_BODY).contains(&body_len) {
        return Err(AppError::bad(
            "invalid_body",
            format!("body must be between {MIN_BODY} and {MAX_BODY} characters"),
        ));
    }
    let mut title =
        spam::sanitize(b.title.as_deref().unwrap_or_default()).replace(['\n', '\t'], " ");
    if title.chars().count() > MAX_TITLE {
        return Err(AppError::bad(
            "invalid_title",
            format!("title must be at most {MAX_TITLE} characters"),
        ));
    }
    if title.is_empty() {
        title = derive_title(&body);
    }
    let contact = spam::sanitize(b.contact.as_deref().unwrap_or_default());
    if contact.chars().count() > MAX_CONTACT {
        return Err(AppError::bad(
            "invalid_contact",
            format!("contact must be at most {MAX_CONTACT} characters"),
        ));
    }
    if let Some(r) = b.rating
        && !(1..=5).contains(&r)
    {
        return Err(AppError::bad(
            "invalid_rating",
            "rating must be between 1 and 5",
        ));
    }
    let meta = match &b.meta {
        None | Some(Value::Null) => "{}".to_string(),
        Some(v @ Value::Object(_)) => {
            let s = v.to_string();
            if s.len() > MAX_META_BYTES {
                return Err(AppError::bad(
                    "invalid_meta",
                    format!("meta must serialize to at most {MAX_META_BYTES} bytes"),
                ));
            }
            s
        }
        Some(_) => return Err(AppError::bad("invalid_meta", "meta must be a JSON object")),
    };
    let diagnostics = match &b.diagnostics {
        None | Some(Value::Null) => "{}".to_string(),
        Some(v @ Value::Object(_)) if v.to_string().len() <= 65536 => v.to_string(),
        Some(_) => {
            return Err(AppError::bad(
                "invalid_diagnostics",
                "diagnostics must be an object of at most 65536 UTF-8 bytes",
            ));
        }
    };
    let source = b.source.as_deref().map(str::trim).unwrap_or("api");
    if source != "api" && source != "web" {
        return Err(AppError::bad(
            "invalid_source",
            "source must be \"api\" or \"web\"",
        ));
    }
    Ok(Clean {
        kind: kind.to_string(),
        title,
        body,
        contact: (!contact.is_empty()).then_some(contact),
        rating: b.rating,
        meta,
        diagnostics,
        source: source.to_string(),
    })
}

fn derive_title(body: &str) -> String {
    let line = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    if line.chars().count() <= DERIVED_TITLE {
        line.to_string()
    } else {
        let cut: String = line.chars().take(DERIVED_TITLE - 3).collect();
        format!("{}...", cut.trim_end())
    }
}

fn nonce_string(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn accepted() -> Response {
    (StatusCode::ACCEPTED, Json(json!({ "ok": true }))).into_response()
}

pub async fn submit(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    JsonBody(body): JsonBody<SubmitBody>,
) -> AppResult<Response> {
    let key = ingest_key(&headers, body.key.as_deref()).ok_or(AppError::NotFound)?;
    let cfg = state.cfg.clone();
    let secret = state.secret.clone();

    let stored = state
        .db
        .call(move |c| -> anyhow::Result<AppResult<bool>> {
            let ts = now();
            let Some((pid, accepting)) = project_by_key(c, &key)? else {
                return Ok(Err(AppError::NotFound));
            };
            if !accepting {
                return Ok(Err(paused()));
            }

            if body.website.as_deref().is_some_and(|w| !w.trim().is_empty()) {
                ratelimit::record_block(c, Some(pid), "honeypot", ts)?;
                return Ok(Ok(false));
            }

            let clean = match validate(&body) {
                Ok(clean) => clean,
                Err(e) => return Ok(Err(e)),
            };

            let bucket = format!("submit:{pid}");
            let l = &cfg.limits;
            let rules = [
                Rule { bucket: &bucket, ip_hash: Some(&ip), window_secs: 60, limit: l.per_ip_project_minute },
                Rule { bucket: &bucket, ip_hash: Some(&ip), window_secs: 3600, limit: l.per_ip_project_hour },
                Rule { bucket: &bucket, ip_hash: Some(&ip), window_secs: 86400, limit: l.per_ip_project_day },
                Rule { bucket: "submit", ip_hash: Some(&ip), window_secs: 86400, limit: l.per_ip_global_day },
                Rule { bucket: &bucket, ip_hash: None, window_secs: 3600, limit: l.per_project_hour },
            ];
            if let Err(retry_after) = ratelimit::check(c, ts, &rules)? {
                ratelimit::record_block(c, Some(pid), "rate_limited", ts)?;
                return Ok(Err(AppError::RateLimited { retry_after }));
            }

            let token = body.challenge.as_deref().unwrap_or_default();
            let nonce = nonce_string(body.nonce.as_ref());
            let challenge = match pow::verify(&secret, token, &nonce, pid, now_ms(), cfg.pow.challenge_ttl_secs) {
                Ok(ch) => ch,
                Err(e) => {
                    ratelimit::record_block(c, Some(pid), e.code(), ts)?;
                    return Ok(Err(AppError::bad(e.code(), pow_message(&e))));
                }
            };
            let first_use = c.execute(
                "INSERT OR IGNORE INTO used_challenges (id, expires_at) VALUES (?1, ?2)",
                params![challenge.id, challenge.issued_ms / 1000 + cfg.pow.challenge_ttl_secs + 120],
            )?;
            if first_use == 0 {
                ratelimit::record_block(c, Some(pid), "challenge_used", ts)?;
                return Ok(Err(AppError::bad("challenge_used", "challenge already used, fetch a new one")));
            }

            ratelimit::record(c, &bucket, &ip, ts)?;
            ratelimit::record(c, "submit", &ip, ts)?;

            let hash = spam::content_hash(&format!("{}:{}", clean.kind, clean.title), &format!("{}\n{}", clean.body, clean.diagnostics));
            let existing: Option<(i64, String)> = c
                .query_row(
                    "SELECT id, ip_hash FROM feedback WHERE project_id = ?1 AND content_hash = ?2
                     AND created_at > ?3 ORDER BY id DESC LIMIT 1",
                    params![pid, hash, ts - 86400],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((orig, orig_ip)) = existing {
                if orig_ip == ip {
                    ratelimit::record_block(c, Some(pid), "duplicate", ts)?;
                } else {
                    // Same report from someone else is a signal, not noise: count it.
                    c.execute(
                        "UPDATE feedback SET dup_count = dup_count + 1, last_seen_at = ?2, updated_at = ?2 WHERE id = ?1",
                        params![orig, ts],
                    )?;
                }
                return Ok(Ok(false));
            }

            let too_fast = now_ms() - challenge.issued_ms < l.min_fill_secs * 1000;
            let (score, signals) = spam::score(&clean.title, &clean.body);
            let over = score >= l.spam_threshold;
            let mut reasons = Vec::new();
            if too_fast {
                reasons.push("too_fast".to_string());
                ratelimit::record_block(c, Some(pid), "too_fast", ts)?;
            }
            if over {
                reasons.push(format!("spam_score ({})", signals.join(", ")));
                ratelimit::record_block(c, Some(pid), "spam_score", ts)?;
            }
            let status = if reasons.is_empty() { "new" } else { "spam" };
            let spam_reason = (!reasons.is_empty()).then(|| reasons.join(", "));

            insert_feedback(c, pid, &clean, status, spam_reason.as_deref(), score as f64, &hash, &ip, ts)?;
            Ok(Ok(true))
        })
        .await??;

    if stored {
        state.embed_notify.notify_one();
    }
    Ok(accepted())
}

#[allow(clippy::too_many_arguments)]
fn insert_feedback(
    c: &Connection,
    pid: i64,
    clean: &Clean,
    status: &str,
    spam_reason: Option<&str>,
    score: f64,
    hash: &str,
    ip_hash: &str,
    ts: i64,
) -> rusqlite::Result<i64> {
    c.execute(
        "INSERT INTO feedback (project_id, kind, title, body, contact, rating, meta, source, status,
            spam_reason, spam_score, content_hash, ip_hash, created_at, updated_at, last_seen_at, diagnostics)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14, ?14, ?15)",
        params![
            pid,
            clean.kind,
            clean.title,
            clean.body,
            clean.contact,
            clean.rating,
            clean.meta,
            clean.source,
            status,
            spam_reason,
            score,
            hash,
            ip_hash,
            ts,
            clean.diagnostics
        ],
    )?;
    Ok(c.last_insert_rowid())
}

/// Feedback created by the admin side (MCP import from Discord, email, ...).
/// Same validation as public submissions, but no PoW, rate limits or spam scoring.
#[derive(Default)]
pub(crate) struct AdminFeedbackInput {
    pub(crate) kind: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) body: Option<String>,
    pub(crate) contact: Option<String>,
    pub(crate) rating: Option<i64>,
    pub(crate) meta: Option<Value>,
    pub(crate) diagnostics: Option<Value>,
    pub(crate) status: Option<String>,
    pub(crate) admin_note: Option<String>,
    pub(crate) created_at: Option<i64>,
}

/// Inserts admin-side feedback with `source = "mcp"` and returns its id.
pub(crate) async fn op_create_feedback(
    state: &AppState,
    project_id: i64,
    input: AdminFeedbackInput,
) -> AppResult<i64> {
    let mut clean = validate(&SubmitBody {
        key: None,
        kind: input.kind,
        title: input.title,
        body: input.body,
        contact: input.contact,
        rating: input.rating,
        meta: input.meta,
        diagnostics: input.diagnostics,
        source: None,
        challenge: None,
        nonce: None,
        website: None,
    })?;
    clean.source = "mcp".to_string();
    let status = match input.status.as_deref() {
        Some(s) => super::admin::validate_status(s)?,
        None => "new".to_string(),
    };
    let note = input.admin_note.unwrap_or_default();
    if note.chars().count() > 10_000 {
        return Err(AppError::bad(
            "invalid_admin_note",
            "admin_note must be at most 10000 characters",
        ));
    }
    let ts = now();
    let created = input.created_at.unwrap_or(ts);
    if created <= 0 || created > ts + 86400 {
        return Err(AppError::bad(
            "invalid_created_at",
            "created_at must be a past Unix timestamp",
        ));
    }
    let id = state
        .db
        .call(move |c| {
            let exists = c
                .query_row(
                    "SELECT 1 FROM projects WHERE id = ?1",
                    params![project_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !exists {
                return Ok(Err(AppError::NotFound));
            }
            let hash = spam::content_hash(&format!("{}:{}", clean.kind, clean.title), &format!("{}\n{}", clean.body, clean.diagnostics));
            let id = insert_feedback(
                c, project_id, &clean, &status, None, 0.0, &hash, "mcp", created,
            )?;
            c.execute(
                // Imports are curated by the admin: skip the near duplicate check.
                "UPDATE feedback SET admin_note = ?2, updated_at = ?3, dup_checked = 1 WHERE id = ?1",
                params![id, note, ts],
            )?;
            Ok(Ok(id))
        })
        .await??;
    state.embed_notify.notify_one();
    Ok(id)
}

fn pow_message(e: &pow::PowError) -> &'static str {
    match e {
        pow::PowError::Malformed => "challenge is missing or malformed",
        pow::PowError::BadSignature => "challenge signature is invalid",
        pow::PowError::WrongProject => "challenge was issued for another project",
        pow::PowError::Expired => "challenge expired, fetch a new one",
        pow::PowError::InsufficientWork => "proof of work nonce is invalid",
    }
}

pub async fn hosted(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> AppResult<Response> {
    let project = state
        .db
        .call(move |c| {
            Ok(c.query_row(
                "SELECT title, description, ingest_key, accepting FROM projects WHERE slug = ?1",
                params![slug],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, bool>(3)?,
                    ))
                },
            )
            .optional()?)
        })
        .await?;
    let Some((title, description, key, accepting)) = project else {
        return Ok((
            StatusCode::NOT_FOUND,
            Html(page(
                &Meta::plain("Not found"),
                "<h1>Not found</h1><p class=\"lead\">No such project.</p>",
            )),
        )
            .into_response());
    };
    let t = html_escape(&title);
    let content = if accepting {
        let desc = if description.trim().is_empty() {
            String::new()
        } else {
            format!("<p class=\"desc\">{}</p>", html_escape(&description))
        };
        format!(
            "<h1>{t}</h1>{desc}<p class=\"lead\">Found a bug or have an idea? Tell us below.</p>\
             <div data-fh-inline data-fh-key=\"{}\" data-fh-project=\"{t}\"></div>\
             <script src=\"/widget.js\" defer></script>",
            html_escape(&key)
        )
    } else {
        format!(
            "<h1>{t}</h1><p class=\"lead\">This project is not accepting feedback right now. Please check back later.</p>"
        )
    };
    let summary = description.split_whitespace().collect::<Vec<_>>().join(" ");
    let meta = Meta {
        title: format!("Send feedback: {title} | Feedback Harvest"),
        og_title: format!("Send feedback: {title}"),
        description: if summary.is_empty() {
            format!("Report a bug or share feedback about {title}.")
        } else {
            summary.chars().take(200).collect()
        },
    };
    let mut resp = Html(page(&meta, &content)).into_response();
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    Ok(resp)
}

/// Head metadata for server rendered pages. Values are escaped by [`page`].
struct Meta {
    title: String,
    og_title: String,
    description: String,
}

impl Meta {
    fn plain(title: &str) -> Self {
        Self {
            title: format!("{title} | Feedback Harvest"),
            og_title: title.to_string(),
            description: "Feedback Harvest".into(),
        }
    }
}

fn page(meta: &Meta, content: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <meta name=\"color-scheme\" content=\"light dark\">\
         <meta name=\"robots\" content=\"noindex, nofollow\">\
         <title>{title}</title><meta name=\"description\" content=\"{desc}\">\
         <meta name=\"theme-color\" content=\"#f9f9f7\" media=\"(prefers-color-scheme: light)\">\
         <meta name=\"theme-color\" content=\"#0d0d0d\" media=\"(prefers-color-scheme: dark)\">\
         <link rel=\"icon\" href=\"/favicon.ico\" sizes=\"48x48\">\
         <link rel=\"icon\" href=\"/favicon.svg\" type=\"image/svg+xml\">\
         <link rel=\"apple-touch-icon\" href=\"/apple-touch-icon.png\">\
         <meta property=\"og:type\" content=\"website\">\
         <meta property=\"og:site_name\" content=\"Feedback Harvest\">\
         <meta property=\"og:title\" content=\"{og}\">\
         <meta property=\"og:description\" content=\"{desc}\">\
         <meta property=\"og:image\" content=\"/admin/og.png\">\
         <meta name=\"twitter:card\" content=\"summary_large_image\"><style>\
         :root{{color-scheme:light dark;font-family:system-ui,-apple-system,Segoe UI,sans-serif;\
         background:#f9f9f7;color:#0b0b0b}}\
         @media (prefers-color-scheme:dark){{:root{{background:#0d0d0d;color:#f0efec}}}}\
         body{{margin:0;padding:2rem 1rem;line-height:1.5}}\
         main{{max-width:40rem;margin:0 auto}}h1{{margin:0 0 .5rem;font-size:1.6rem}}\
         .desc{{opacity:.8;white-space:pre-line}}.lead{{margin:1rem 0}}\
         footer{{max-width:40rem;margin:2.5rem auto 0;font-size:.85rem;opacity:.7}}\
         footer a{{color:inherit;text-decoration:none;display:inline-flex;align-items:center;gap:.4rem}}\
         </style></head><body><main>{content}</main>\
         <footer><a href=\"https://github.com/georgevibing/feedback-harvest\" rel=\"noopener\">\
         <img src=\"/favicon.svg\" alt=\"\" width=\"16\" height=\"16\">Powered by Feedback Harvest</a></footer>\
         </body></html>",
        title = html_escape(&meta.title),
        og = html_escape(&meta.og_title),
        desc = html_escape(&meta.description),
    )
}

#[cfg(test)]
mod tests {
    use super::derive_title;

    #[test]
    fn derives_title_from_first_line() {
        assert_eq!(derive_title("\n  hello world \nmore"), "hello world");
        let long = "x".repeat(200);
        let t = derive_title(&long);
        assert_eq!(t.chars().count(), 80);
        assert!(t.ends_with("..."));
    }
}
