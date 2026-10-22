//! MCP tools. Each tool validates its arguments, calls the shared admin
//! operation and returns `structuredContent` plus a compact text rendering.

use std::collections::HashMap;

use rusqlite::{OptionalExtension, params};
use serde_json::{Map, Value, json};

use super::{Ctx, RpcError};
use crate::{
    db::now,
    embed,
    error::AppError,
    routes::{
        admin::{self, BulkBody, FeedbackPatch, ProjectInput},
        labels::{self, AssignInput, LabelInput},
        public::{self, AdminFeedbackInput},
    },
    tokens::Scope,
};

const MAX_EXPORT: i64 = 1000;
const SNIPPET: usize = 180;

struct ToolDef {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    input: Value,
    output: Option<Value>,
    write: bool,
    destructive: bool,
    idempotent: bool,
}

// ---------------------------------------------------------------------------
// Schemas

fn schema(props: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    })
}

fn project_prop() -> Value {
    json!({ "type": "string", "description": "Project id (e.g. \"3\") or slug (e.g. \"my-game\")." })
}

fn filter_props(default_status: &str) -> Map<String, Value> {
    let props = json!({
        "project": { "type": "string", "description": "Restrict to one project: id or slug. Omit for all projects." },
        "kind": { "type": "string", "enum": ["bug", "feedback"], "description": "Restrict to bug reports or general feedback." },
        "label_ids": { "type": "array", "items": { "type": "integer", "minimum": 1 }, "maxItems": 20, "description": "Filter by project-scoped label ids from list_labels. Combines with text, status, project and date filters." },
        "label_match": { "type": "string", "enum": ["all", "any"], "description": "all (default) requires every selected label; any requires at least one." },
        "unlabeled": { "type": "boolean", "description": "Only feedback without labels. Cannot be combined with label_ids." },
        "status": {
            "type": "string",
            "enum": ["open", "all", "any", "new", "triaged", "resolved", "archived", "spam"],
            "description": format!("open = new + triaged, all = everything except spam, any = everything including spam, or one concrete status. Default {default_status}."),
        },
        "from": { "type": "string", "description": "Only items created at or after this time: ISO 8601 date/datetime (\"2026-10-01\") or Unix seconds." },
        "to": { "type": "string", "description": "Only items created at or before this time: ISO 8601 date/datetime (a bare date means end of that day) or Unix seconds." },
        "days": { "type": "integer", "minimum": 1, "maximum": 3650, "description": "Shortcut for from = now minus N days." },
        "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "Page size, default 50." },
        "offset": { "type": "integer", "minimum": 0, "description": "Items to skip, use next_offset from the previous page." },
    });
    match props {
        Value::Object(m) => m,
        _ => unreachable!(),
    }
}

fn feedback_page_output() -> Value {
    json!({
        "type": "object",
        "properties": {
            "items": { "type": "array", "items": { "type": "object" } },
            "total": { "type": "integer" },
            "mode": { "type": "string" },
            "semantic": { "type": "boolean" },
            "next_offset": { "type": ["integer", "null"] },
        },
        "required": ["items", "total"],
    })
}

fn object_output() -> Value {
    json!({ "type": "object" })
}

fn defs() -> Vec<ToolDef> {
    let project_fields = json!({
        "title": { "type": "string", "maxLength": 120, "description": "Display name, e.g. \"Starfall (Steam)\"." },
        "slug": { "type": "string", "pattern": "^[a-z0-9][a-z0-9-]{0,62}$", "description": "URL id used in /s/<slug>. Derived from the title when omitted on create." },
        "project_type": { "type": "string", "enum": crate::models::PROJECT_TYPES, "description": "What kind of product this is." },
        "description": { "type": "string", "maxLength": 2000, "description": "Shown on the hosted feedback page." },
        "accepting": { "type": "boolean", "description": "false pauses new submissions (challenge and submit answer 403)." },
    });
    let mut update_fields = project_fields.as_object().cloned().unwrap_or_default();
    update_fields.insert("project".into(), project_prop());

    let mut list_props = filter_props("open");
    list_props.insert(
        "q".into(),
        json!({ "type": "string", "description": "Optional search text. When set, results are ordered by relevance (hybrid search)." }),
    );
    let mut search_props = filter_props("all");
    search_props.insert(
        "q".into(),
        json!({ "type": "string", "minLength": 1, "description": "What to look for: words, a phrase with typos, or a description of the problem." }),
    );
    search_props.insert(
        "mode".into(),
        json!({
            "type": "string",
            "enum": ["hybrid", "keyword", "fuzzy", "semantic"],
            "description": "hybrid (default) fuses all signals and is right almost always. keyword: exact stemmed words. fuzzy: typos and partial words. semantic: meaning, finds paraphrases without shared words.",
        }),
    );

    vec![
        ToolDef {
            name: "list_labels",
            title: "List labels and theme counts",
            description: "List project-scoped labels, descriptions, colors and counts (open_count = new + triaged; feedback_count excludes spam). Sorted by open_count. Read before creating labels to reuse the owner's vocabulary. Counts overlap for multi-label feedback.",
            input: schema(json!({"project": project_prop()}), &[]),
            output: Some(object_output()),
            write: false,
            destructive: false,
            idempotent: true,
        },
        ToolDef {
            name: "create_label",
            title: "Create a project label",
            description: "Create a reusable theme in one project. Prefer a concise name and a description defining when it applies. Reuse existing labels from list_labels; names are unique per project after lowercasing. On conflict, list labels again rather than retrying with a near-duplicate name.",
            input: schema(
                json!({"project": project_prop(), "name": {"type":"string", "minLength":1, "maxLength":64}, "description":{"type":"string", "maxLength":500}, "color":{"type":"string", "enum": labels::COLORS}}),
                &["project", "name"],
            ),
            output: Some(object_output()),
            write: true,
            destructive: false,
            idempotent: false,
        },
        ToolDef {
            name: "update_label",
            title: "Update a label definition",
            description: "Rename a label, change its color or refine its description. Keeps the label id and every feedback assignment. Labels cannot move between projects. Use to consolidate terminology without relabeling feedback.",
            input: schema(
                json!({"id":{"type":"integer", "minimum":1}, "name":{"type":"string", "minLength":1, "maxLength":64}, "description":{"type":"string", "maxLength":500}, "color":{"type":"string", "enum": labels::COLORS}}),
                &["id"],
            ),
            output: Some(object_output()),
            write: true,
            destructive: false,
            idempotent: true,
        },
        ToolDef {
            name: "delete_label",
            title: "Delete a label",
            description: "Delete a label definition and its assignments, preserving all feedback. Requires confirm=true and explicit owner intent. To merge labels, add the destination label to every source item (paginate), verify, and only then delete the source label.",
            input: schema(
                json!({"id":{"type":"integer", "minimum":1}, "confirm":{"type":"boolean", "description":"Must be true; removes this label from all feedback."}}),
                &["id", "confirm"],
            ),
            output: Some(object_output()),
            write: true,
            destructive: true,
            idempotent: false,
        },
        ToolDef {
            name: "label_feedback",
            title: "Assign labels to feedback",
            description: "Atomically add, remove or set labels on 1..500 feedback ids. Every label must belong to every target item's project. Prefer add/remove to preserve unrelated labels and concurrent classification; both are idempotent. set replaces the complete set, and set [] clears it. Max 20 labels per item. Unknown ids or scope errors cause no changes. affected counts changed items.",
            input: schema(
                json!({"ids":{"type":"array", "items":{"type":"integer", "minimum":1}, "minItems":1, "maxItems":500}, "label_ids":{"type":"array", "items":{"type":"integer", "minimum":1}, "maxItems":20}, "action":{"type":"string", "enum":["add","remove","set"]}}),
                &["ids", "label_ids", "action"],
            ),
            output: Some(object_output()),
            write: true,
            destructive: false,
            idempotent: true,
        },
        ToolDef {
            name: "list_projects",
            title: "List projects",
            description: "List every project with its counters (total excluding spam, open, new, bugs, feedback, spam, last_at). Start here to learn project ids and slugs.",
            input: schema(json!({}), &[]),
            output: Some(
                json!({ "type": "object", "properties": { "projects": { "type": "array", "items": { "type": "object" } } }, "required": ["projects"] }),
            ),
            write: false,
            destructive: false,
            idempotent: true,
        },
        ToolDef {
            name: "get_project",
            title: "Get project",
            description: "Get one project by id or slug, including its ingest key and counters.",
            input: schema(json!({ "project": project_prop() }), &["project"]),
            output: Some(object_output()),
            write: false,
            destructive: false,
            idempotent: true,
        },
        ToolDef {
            name: "list_feedback",
            title: "List feedback",
            description: "Browse feedback newest first with filters (project, kind, status, date range). Default status is open (new + triaged). Use status=new for the untriaged inbox. Pass q to search instead (relevance order). Paginate with offset/next_offset.",
            input: schema(Value::Object(list_props), &[]),
            output: Some(feedback_page_output()),
            write: false,
            destructive: false,
            idempotent: true,
        },
        ToolDef {
            name: "search_feedback",
            title: "Search feedback",
            description: "Search feedback by text with relevance ranking. Default mode hybrid (keyword + fuzzy + semantic fused with reciprocal rank fusion) and default status all (everything except spam). Use mode=semantic for 'reports about X' questions, fuzzy for half-remembered words, keyword for exact error strings. Response says whether vectors were used (semantic=false means lexical only).",
            input: schema(Value::Object(search_props), &["q"]),
            output: Some(feedback_page_output()),
            write: false,
            destructive: false,
            idempotent: true,
        },
        ToolDef {
            name: "get_feedback",
            title: "Get feedback item",
            description: "Get one feedback item in full (body, meta, contact, notes, spam info) plus up to 5 semantically similar items from the same project, which is how you spot duplicates.",
            input: schema(
                json!({ "id": { "type": "integer", "description": "Feedback id." } }),
                &["id"],
            ),
            output: Some(
                json!({ "type": "object", "properties": { "item": { "type": "object" }, "similar": { "type": "array", "items": { "type": "object" } } }, "required": ["item", "similar"] }),
            ),
            write: false,
            destructive: false,
            idempotent: true,
        },
        ToolDef {
            name: "find_similar_feedback",
            title: "Find similar feedback",
            description: "Find feedback similar in meaning to an existing item (id) or to free text (text), e.g. to check whether a bug was already reported before creating or triaging. Excludes spam. Falls back to hybrid text search when embeddings are unavailable.",
            input: schema(
                json!({
                    "id": { "type": "integer", "description": "Find items similar to this feedback id." },
                    "text": { "type": "string", "description": "Or: find items similar to this text." },
                    "project": { "type": "string", "description": "Restrict to a project (id or slug). Defaults to the item's project when id is given." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Default 10." },
                }),
                &[],
            ),
            output: Some(
                json!({ "type": "object", "properties": { "items": { "type": "array", "items": { "type": "object" } }, "semantic": { "type": "boolean" } }, "required": ["items"] }),
            ),
            write: false,
            destructive: false,
            idempotent: true,
        },
        ToolDef {
            name: "get_stats",
            title: "Get stats",
            description: "Dashboard numbers for all projects or one: totals, last 7 days, a 30 day daily series of bugs vs feedback, anti-spam block counts by reason (30 days) and embedding status.",
            input: schema(
                json!({ "project": { "type": "string", "description": "Project id or slug. Omit for all projects." } }),
                &[],
            ),
            output: Some(object_output()),
            write: false,
            destructive: false,
            idempotent: true,
        },
        ToolDef {
            name: "export_feedback",
            title: "Export feedback",
            description: "Export all feedback of a project (every status, oldest first) as JSON objects or CSV text, paginated (limit up to 1000). Use for analysis or backups; prefer list/search for browsing.",
            input: schema(
                json!({
                    "project": project_prop(),
                    "format": { "type": "string", "enum": ["json", "csv"], "description": "Default json." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": MAX_EXPORT, "description": "Default 200." },
                    "offset": { "type": "integer", "minimum": 0 },
                }),
                &["project"],
            ),
            output: Some(
                json!({ "type": "object", "properties": { "project": { "type": "string" }, "total": { "type": "integer" }, "offset": { "type": "integer" }, "next_offset": { "type": ["integer", "null"] }, "items": { "type": "array" }, "csv": { "type": "string" } }, "required": ["project", "total"] }),
            ),
            write: false,
            destructive: false,
            idempotent: true,
        },
        ToolDef {
            name: "get_integration_snippets",
            title: "Get integration snippets",
            description: "Ready to paste integration code for a project: hosted feedback page URL, floating widget script tag, inline embed, ESM client usage and a curl walkthrough (challenge, solve, submit). Use when wiring feedback into a game, site or service.",
            input: schema(
                json!({
                    "project": project_prop(),
                    "base_url": { "type": "string", "description": "Public URL of this server, e.g. https://feedback.example.com. Defaults to the URL this request came in on." },
                }),
                &["project"],
            ),
            output: Some(object_output()),
            write: false,
            destructive: false,
            idempotent: true,
        },
        ToolDef {
            name: "create_project",
            title: "Create project",
            description: "Create a project to collect feedback for. Returns the project with its public ingest key. Follow up with get_integration_snippets.",
            input: schema(project_fields.clone(), &["title"]),
            output: Some(object_output()),
            write: true,
            destructive: false,
            idempotent: false,
        },
        ToolDef {
            name: "update_project",
            title: "Update project",
            description: "Change a project's title, slug, type, description or accepting flag (pause or resume submissions). Only the fields you pass change. Changing the slug changes the hosted page URL.",
            input: schema(Value::Object(update_fields), &["project"]),
            output: Some(object_output()),
            write: true,
            destructive: false,
            idempotent: true,
        },
        ToolDef {
            name: "rotate_project_key",
            title: "Rotate ingest key",
            description: "Replace a project's ingest key. The old key stops working immediately, so every embedded client must be updated. Use when a key is being abused.",
            input: schema(json!({ "project": project_prop() }), &["project"]),
            output: Some(object_output()),
            write: true,
            destructive: true,
            idempotent: false,
        },
        ToolDef {
            name: "delete_project",
            title: "Delete project",
            description: "Permanently delete a project and ALL its feedback. Irreversible. Only on explicit user request; requires confirm_slug equal to the project's slug. Prefer update_project accepting=false to pause.",
            input: schema(
                json!({
                    "project": project_prop(),
                    "confirm_slug": { "type": "string", "description": "Must equal the project's slug." },
                }),
                &["project", "confirm_slug"],
            ),
            output: Some(object_output()),
            write: true,
            destructive: true,
            idempotent: false,
        },
        ToolDef {
            name: "create_feedback",
            title: "Create feedback",
            description: "Add a feedback item on the admin side, for example when importing reports from Discord, email or a store review. Bypasses anti-spam; source is recorded as 'mcp'. Run find_similar_feedback first to avoid duplicates.",
            input: schema(
                json!({
                    "project": project_prop(),
                    "kind": { "type": "string", "enum": ["bug", "feedback"] },
                    "body": { "type": "string", "minLength": 3, "maxLength": 10000, "description": "The report itself. Keep the reporter's words." },
                    "title": { "type": "string", "maxLength": 200, "description": "Short summary. Derived from the first line of body when omitted." },
                    "contact": { "type": "string", "maxLength": 200, "description": "How to reach the reporter, if known." },
                    "rating": { "type": "integer", "minimum": 1, "maximum": 5 },
                    "meta": { "type": "object", "description": "Extra context, e.g. {\"version\": \"1.2.3\", \"platform\": \"steam-deck\", \"origin\": \"discord\"}. Max 4 KB." },
                    "status": { "type": "string", "enum": crate::models::STATUSES, "description": "Default new." },
                    "admin_note": { "type": "string", "maxLength": 10000 },
                    "created_at": { "type": "string", "description": "When it was originally reported: ISO 8601 or Unix seconds. Default now." },
                }),
                &["project", "kind", "body"],
            ),
            output: Some(object_output()),
            write: true,
            destructive: false,
            idempotent: false,
        },
        ToolDef {
            name: "update_feedback",
            title: "Update feedback",
            description: "Triage one item: change status (new, triaged, resolved, archived, spam), switch kind, replace admin_note or append_note (adds a dated line, keeps history). Moving an item out of spam clears its spam_reason.",
            input: schema(
                json!({
                    "id": { "type": "integer" },
                    "status": { "type": "string", "enum": crate::models::STATUSES },
                    "kind": { "type": "string", "enum": ["bug", "feedback"] },
                    "admin_note": { "type": "string", "maxLength": 10000, "description": "Replaces the private note." },
                    "append_note": { "type": "string", "maxLength": 2000, "description": "Appended to the note as a new dated line. Cannot be combined with admin_note." },
                }),
                &["id"],
            ),
            output: Some(object_output()),
            write: true,
            destructive: false,
            idempotent: true,
        },
        ToolDef {
            name: "bulk_update_feedback",
            title: "Bulk update status",
            description: "Set the same status on up to 500 feedback ids at once, e.g. archive a batch of duplicates or resolve everything fixed in a release.",
            input: schema(
                json!({
                    "ids": { "type": "array", "items": { "type": "integer" }, "minItems": 1, "maxItems": 500 },
                    "status": { "type": "string", "enum": crate::models::STATUSES },
                }),
                &["ids", "status"],
            ),
            output: Some(
                json!({ "type": "object", "properties": { "affected": { "type": "integer" } }, "required": ["affected"] }),
            ),
            write: true,
            destructive: false,
            idempotent: true,
        },
        ToolDef {
            name: "delete_feedback",
            title: "Delete feedback",
            description: "Permanently delete one feedback item. Irreversible: prefer status archived or spam. Requires confirm=true.",
            input: schema(
                json!({
                    "id": { "type": "integer" },
                    "confirm": { "type": "boolean", "description": "Must be true." },
                }),
                &["id", "confirm"],
            ),
            output: Some(object_output()),
            write: true,
            destructive: true,
            idempotent: false,
        },
        ToolDef {
            name: "reindex_embeddings",
            title: "Rebuild semantic index",
            description: "Drop all vectors and re-embed every item in the background. Only needed after switching the embedding provider or model; progress shows in get_stats (embeddings.pending).",
            input: schema(json!({}), &[]),
            output: Some(object_output()),
            write: true,
            destructive: false,
            idempotent: true,
        },
    ]
}

pub fn list(scope: Scope) -> Vec<Value> {
    defs()
        .into_iter()
        .filter(|d| scope.can_write() || !d.write)
        .map(|d| {
            let mut t = json!({
                "name": d.name,
                "title": d.title,
                "description": d.description,
                "inputSchema": d.input,
                "annotations": {
                    "title": d.title,
                    "readOnlyHint": !d.write,
                    "destructiveHint": d.destructive,
                    "idempotentHint": d.idempotent,
                    "openWorldHint": false,
                },
            });
            if let Some(o) = d.output {
                t["outputSchema"] = o;
            }
            t
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Argument helpers. Validation failures become tool errors so the model can
// correct itself (MCP 2025-11-25 guidance).

type ToolResult = Result<(Value, String), String>;

fn app_err(e: AppError) -> String {
    match e {
        AppError::BadRequest(code, msg) | AppError::Forbidden(code, msg) => {
            format!("{code}: {msg}")
        }
        AppError::NotFound => "not_found: no such item".into(),
        AppError::Conflict(msg) => format!("conflict: {msg}"),
        AppError::RateLimited { retry_after } => format!("rate_limited: retry in {retry_after}s"),
        AppError::Unauthorized | AppError::InvalidCredentials => "unauthorized".into(),
        AppError::Internal(e) => {
            tracing::error!(error = ?e, "mcp tool internal error");
            "internal error".into()
        }
    }
}

struct Args<'a>(&'a Map<String, Value>);

impl Args<'_> {
    fn has(&self, k: &str) -> bool {
        self.0.get(k).is_some_and(|v| !v.is_null())
    }

    fn str(&self, k: &str) -> Result<Option<String>, String> {
        match self.0.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(Value::Number(n)) => Ok(Some(n.to_string())),
            Some(_) => Err(format!("invalid_arguments: {k} must be a string")),
        }
    }

    fn req_str(&self, k: &str) -> Result<String, String> {
        self.str(k)?
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| format!("invalid_arguments: {k} is required"))
    }

    fn int(&self, k: &str) -> Result<Option<i64>, String> {
        match self.0.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Number(n)) => n
                .as_i64()
                .map(Some)
                .ok_or_else(|| format!("invalid_arguments: {k} must be an integer")),
            Some(Value::String(s)) => s
                .trim()
                .parse()
                .map(Some)
                .map_err(|_| format!("invalid_arguments: {k} must be an integer")),
            Some(_) => Err(format!("invalid_arguments: {k} must be an integer")),
        }
    }

    fn req_int(&self, k: &str) -> Result<i64, String> {
        self.int(k)?
            .ok_or_else(|| format!("invalid_arguments: {k} is required"))
    }

    fn ints(&self, k: &str) -> Result<Option<Vec<i64>>, String> {
        match self.0.get(k) {
            None => Ok(None),
            Some(Value::Array(v)) => v
                .iter()
                .map(|x| {
                    x.as_i64().ok_or_else(|| {
                        format!("invalid_arguments: {k} must be an array of integers")
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some),
            _ => Err(format!(
                "invalid_arguments: {k} must be an array of integers"
            )),
        }
    }

    fn bool(&self, k: &str) -> Result<Option<bool>, String> {
        match self.0.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(Value::String(s)) if s == "true" || s == "false" => Ok(Some(s == "true")),
            Some(_) => Err(format!("invalid_arguments: {k} must be a boolean")),
        }
    }

    /// Unix seconds or ISO 8601. A bare date resolves to the start of the day, or
    /// the end of the day when `end_of_day` (for inclusive upper bounds).
    fn time(&self, k: &str, end_of_day: bool) -> Result<Option<i64>, String> {
        let Some(raw) = self.str(k)? else {
            return Ok(None);
        };
        parse_time(raw.trim(), end_of_day)
            .map(Some)
            .ok_or_else(|| format!("invalid_arguments: {k} must be ISO 8601 (2026-10-01 or 2026-10-01T12:00:00Z) or Unix seconds"))
    }
}

pub fn parse_time(raw: &str, end_of_day: bool) -> Option<i64> {
    use time::{Date, Month, OffsetDateTime, format_description::well_known::Rfc3339};
    if let Ok(n) = raw.parse::<i64>() {
        return Some(n);
    }
    if let Ok(t) = OffsetDateTime::parse(raw, &Rfc3339) {
        return Some(t.unix_timestamp());
    }
    let mut parts = raw.splitn(3, '-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let month = Month::try_from(m.parse::<u8>().ok()?).ok()?;
    let date = Date::from_calendar_date(y.parse().ok()?, month, d.parse().ok()?).ok()?;
    let start = date.midnight().assume_utc().unix_timestamp();
    Some(if end_of_day { start + 86399 } else { start })
}

/// Resolves a project reference (id or slug) to an id.
async fn project_id(ctx: &Ctx, reference: &str) -> Result<i64, String> {
    let r = reference.trim().to_string();
    let by_id = r.parse::<i64>().ok();
    let found = ctx
        .state
        .db
        .call(move |c| {
            Ok(c.query_row(
                "SELECT id FROM projects WHERE id = ?1 OR slug = ?2 ORDER BY id = ?1 DESC LIMIT 1",
                params![by_id, r],
                |row| row.get::<_, i64>(0),
            )
            .optional()?)
        })
        .await
        .map_err(|e| app_err(AppError::Internal(e)))?;
    found.ok_or_else(|| {
        format!("not_found: no project with id or slug {reference:?} (call list_projects)")
    })
}

async fn opt_project(ctx: &Ctx, a: &Args<'_>) -> Result<Option<i64>, String> {
    match a.str("project")?.filter(|s| !s.trim().is_empty()) {
        Some(p) => Ok(Some(project_id(ctx, &p).await?)),
        None => Ok(None),
    }
}

/// Builds the admin list query map from tool arguments.
async fn feedback_query(
    ctx: &Ctx,
    a: &Args<'_>,
    default_status: &str,
) -> Result<HashMap<String, String>, String> {
    let mut q = HashMap::new();
    if let Some(ids) = a.ints("label_ids")? {
        q.insert(
            "label_ids".into(),
            ids.iter().map(i64::to_string).collect::<Vec<_>>().join(","),
        );
    }
    if let Some(v) = a.str("label_match")? {
        q.insert("label_match".into(), v);
    }
    if let Some(v) = a.bool("unlabeled")? {
        q.insert("unlabeled".into(), v.to_string());
    }
    if let Some(p) = opt_project(ctx, a).await? {
        q.insert("project_id".into(), p.to_string());
    }
    if let Some(k) = a.str("kind")? {
        q.insert("kind".into(), k);
    }
    q.insert(
        "status".into(),
        a.str("status")?
            .unwrap_or_else(|| default_status.to_string()),
    );
    let from = match a.int("days")? {
        Some(d) if d >= 1 => Some(now() - d * 86400),
        Some(_) => return Err("invalid_arguments: days must be at least 1".into()),
        None => a.time("from", false)?,
    };
    if let Some(f) = from {
        q.insert("from".into(), f.to_string());
    }
    if let Some(t) = a.time("to", true)? {
        q.insert("to".into(), t.to_string());
    }
    for k in ["limit", "offset"] {
        if let Some(v) = a.int(k)? {
            q.insert(k.into(), v.to_string());
        }
    }
    if let Some(text) = a.str("q")? {
        q.insert("q".into(), text);
    }
    if let Some(m) = a.str("mode")? {
        q.insert("mode".into(), m);
    }
    Ok(q)
}

// ---------------------------------------------------------------------------
// Text rendering

fn snippet(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!(
            "{}...",
            flat.chars().take(max).collect::<String>().trim_end()
        )
    }
}

fn feedback_line(f: &Value, slugs: &HashMap<i64, String>) -> String {
    let project = f["project_id"]
        .as_i64()
        .and_then(|id| slugs.get(&id))
        .map(String::as_str)
        .unwrap_or("?");
    let mut line = format!(
        "- #{} [{}/{}] {} ({project}",
        f["id"],
        f["kind"].as_str().unwrap_or_default(),
        f["status"].as_str().unwrap_or_default(),
        f["title"].as_str().unwrap_or_default(),
    );
    if let Some(ts) = f["created_at"].as_i64() {
        line.push_str(&format!(", {}", date(ts)));
    }
    if let Some(r) = f["rating"].as_i64() {
        line.push_str(&format!(", rating {r}/5"));
    }
    if let Some(d) = f["dup_count"].as_i64().filter(|d| *d > 0) {
        line.push_str(&format!(", +{d} duplicates"));
    }
    if let Some(s) = f["score"].as_f64() {
        line.push_str(&format!(", score {s:.3}"));
    }
    if let Some(labels) = f["labels"].as_array().filter(|v| !v.is_empty()) {
        let names = labels
            .iter()
            .filter_map(|l| l["name"].as_str())
            .collect::<Vec<_>>()
            .join(", ");
        line.push_str(&format!(", labels: {names}"));
    }
    line.push(')');
    if let Some(reason) = f["spam_reason"].as_str() {
        line.push_str(&format!(" spam: {reason}"));
    }
    line.push_str(&format!(
        "\n  {}",
        snippet(f["body"].as_str().unwrap_or_default(), SNIPPET)
    ));
    line
}

fn date(ts: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp(ts)
        .map(|t| format!("{:04}-{:02}-{:02}", t.year(), t.month() as u8, t.day()))
        .unwrap_or_default()
}

fn project_line(p: &Value) -> String {
    let s = &p["stats"];
    format!(
        "- {} (id {}, {}{}): {}. open {}, new {}, bugs {}, feedback {}, spam {}",
        p["slug"].as_str().unwrap_or_default(),
        p["id"],
        p["project_type"].as_str().unwrap_or_default(),
        if p["accepting"].as_bool() == Some(false) {
            ", paused"
        } else {
            ""
        },
        p["title"].as_str().unwrap_or_default(),
        s["open"],
        s["new"],
        s["bugs"],
        s["feedback"],
        s["spam"],
    )
}

async fn slug_map(ctx: &Ctx) -> Result<HashMap<i64, String>, String> {
    ctx.state
        .db
        .call(|c| {
            let mut stmt = c.prepare("SELECT id, slug FROM projects")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<HashMap<i64, String>>>()?)
        })
        .await
        .map_err(|e| app_err(AppError::Internal(e)))
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

fn to_value<T: serde::Serialize>(v: T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| app_err(AppError::Internal(e.into())))
}

async fn render_page(ctx: &Ctx, mut page: Value, offset: i64) -> ToolResult {
    let slugs = slug_map(ctx).await?;
    let items = page["items"].as_array().cloned().unwrap_or_default();
    let total = page["total"].as_i64().unwrap_or(0);
    let next = offset + items.len() as i64;
    page["next_offset"] = if next < total {
        json!(next)
    } else {
        Value::Null
    };
    let mut text = format!(
        "{} of {total} item(s){}",
        items.len(),
        if offset > 0 {
            format!(" from offset {offset}")
        } else {
            String::new()
        }
    );
    if let Some(mode) = page["mode"].as_str()
        && page.get("semantic").is_some()
    {
        text.push_str(&format!(", mode {mode}, semantic {}", page["semantic"]));
    }
    text.push('\n');
    for f in &items {
        text.push_str(&feedback_line(f, &slugs));
        text.push('\n');
    }
    if let Some(n) = page["next_offset"].as_i64() {
        text.push_str(&format!("More available: call again with offset {n}.\n"));
    }
    Ok((page, text))
}

// ---------------------------------------------------------------------------
// Dispatch

pub async fn call(ctx: &Ctx, params: &Value) -> Result<Value, RpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("tools/call needs a tool name"))?;
    let empty = Map::new();
    let args = match params.get("arguments") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(m)) => m,
        Some(_) => return Err(RpcError::invalid_params("arguments must be an object")),
    };
    let Some(def) = defs().into_iter().find(|d| d.name == name) else {
        return Err(RpcError::invalid_params(format!("unknown tool: {name}")));
    };
    let outcome = if def.write && !ctx.token.scope.can_write() {
        Err(format!(
            "forbidden: {name} needs a write scoped token, this token is read only. Create a write token in Admin > API tokens."
        ))
    } else {
        run(ctx, name, &Args(args)).await
    };
    Ok(match outcome {
        Ok((structured, text)) => json!({
            "content": [{ "type": "text", "text": text }],
            "structuredContent": structured,
            "isError": false,
        }),
        Err(message) => json!({
            "content": [{ "type": "text", "text": message }],
            "isError": true,
        }),
    })
}

async fn run(ctx: &Ctx, name: &str, a: &Args<'_>) -> ToolResult {
    let state = &ctx.state;
    match name {
        "list_labels" => {
            let project = opt_project(ctx, a).await?;
            let labels = to_value(
                labels::op_list_labels(state, project)
                    .await
                    .map_err(app_err)?,
            )?;
            Ok((json!({"labels": labels}), pretty(&labels)))
        }
        "create_label" => {
            let project = project_id(ctx, &a.req_str("project")?).await?;
            let v = to_value(
                labels::op_create_label(
                    state,
                    LabelInput {
                        project_id: Some(project),
                        name: Some(a.req_str("name")?),
                        color: a.str("color")?,
                        description: a.str("description")?,
                    },
                )
                .await
                .map_err(app_err)?,
            )?;
            Ok((v.clone(), pretty(&v)))
        }
        "update_label" => {
            let v = to_value(
                labels::op_update_label(
                    state,
                    a.req_int("id")?,
                    LabelInput {
                        project_id: None,
                        name: a.str("name")?,
                        color: a.str("color")?,
                        description: a.str("description")?,
                    },
                )
                .await
                .map_err(app_err)?,
            )?;
            Ok((v.clone(), pretty(&v)))
        }
        "delete_label" => {
            if a.bool("confirm")? != Some(true) {
                return Err("confirmation_required: pass confirm=true to delete the label and its assignments; feedback is preserved".into());
            }
            let id = a.req_int("id")?;
            labels::op_delete_label(state, id).await.map_err(app_err)?;
            Ok((
                json!({"deleted":true,"id":id}),
                format!("Deleted label #{id}; feedback preserved."),
            ))
        }
        "label_feedback" => {
            let v = labels::op_assign_labels(
                state,
                AssignInput {
                    ids: a.ints("ids")?.ok_or("invalid_arguments: ids is required")?,
                    label_ids: a
                        .ints("label_ids")?
                        .ok_or("invalid_arguments: label_ids is required")?,
                    action: a.req_str("action")?,
                },
            )
            .await
            .map_err(app_err)?;
            Ok((
                v.clone(),
                format!("Updated labels on {} item(s).", v["affected"]),
            ))
        }
        "list_projects" => {
            let projects = to_value(admin::op_list_projects(state).await.map_err(app_err)?)?;
            let list = projects.as_array().cloned().unwrap_or_default();
            let mut text = format!("{} project(s)\n", list.len());
            for p in &list {
                text.push_str(&project_line(p));
                text.push('\n');
            }
            Ok((json!({ "projects": projects }), text))
        }
        "get_project" => {
            let id = project_id(ctx, &a.req_str("project")?).await?;
            let p = to_value(admin::op_get_project(state, id).await.map_err(app_err)?)?;
            Ok((p.clone(), pretty(&p)))
        }
        "list_feedback" | "search_feedback" => {
            let default_status = if name == "list_feedback" {
                "open"
            } else {
                "all"
            };
            if name == "search_feedback" {
                a.req_str("q")?;
            }
            let q = feedback_query(ctx, a, default_status).await?;
            let offset = a.int("offset")?.unwrap_or(0).max(0);
            let page = admin::op_list_feedback(state, &q).await.map_err(app_err)?;
            render_page(ctx, page, offset).await
        }
        "get_feedback" => {
            let id = a.req_int("id")?;
            let v = admin::op_get_feedback(state, id)
                .await
                .map_err(|e| match e {
                    AppError::NotFound => format!("not_found: no feedback with id {id}"),
                    e => app_err(e),
                })?;
            let slugs = slug_map(ctx).await?;
            let mut text = pretty(&v["item"]);
            let similar = v["similar"].as_array().cloned().unwrap_or_default();
            if !similar.is_empty() {
                text.push_str("\n\nSimilar items:\n");
                for f in &similar {
                    text.push_str(&feedback_line(f, &slugs));
                    text.push('\n');
                }
            }
            Ok((v, text))
        }
        "find_similar_feedback" => find_similar(ctx, a).await,
        "get_stats" => {
            let project = opt_project(ctx, a).await?;
            let v = admin::op_stats(state, project).await.map_err(app_err)?;
            Ok((v.clone(), pretty(&v)))
        }
        "export_feedback" => {
            let id = project_id(ctx, &a.req_str("project")?).await?;
            let format = a.str("format")?.unwrap_or_else(|| "json".into());
            if format != "json" && format != "csv" {
                return Err("invalid_arguments: format must be json or csv".into());
            }
            let limit = a.int("limit")?.unwrap_or(200).clamp(1, MAX_EXPORT) as usize;
            let offset = a.int("offset")?.unwrap_or(0).max(0) as usize;
            let (slug, items) = admin::op_export_items(state, id).await.map_err(app_err)?;
            let total = items.len();
            let page: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
            let next = offset + page.len();
            let next_offset = if next < total {
                json!(next)
            } else {
                Value::Null
            };
            if format == "csv" {
                let csv = admin::to_csv(&page);
                Ok((
                    json!({ "project": slug, "total": total, "offset": offset, "next_offset": next_offset, "csv": csv }),
                    csv,
                ))
            } else {
                let items = to_value(&page)?;
                let text = format!(
                    "{} of {total} item(s) of {slug} from offset {offset}\n{}",
                    page.len(),
                    serde_json::to_string(&items).unwrap_or_default()
                );
                Ok((
                    json!({ "project": slug, "total": total, "offset": offset, "next_offset": next_offset, "items": items }),
                    text,
                ))
            }
        }
        "get_integration_snippets" => snippets(ctx, a).await,
        "create_project" => {
            let input = project_input(a)?;
            if input.title.is_none() {
                return Err("invalid_arguments: title is required".into());
            }
            let p = to_value(
                admin::op_create_project(state, input)
                    .await
                    .map_err(app_err)?,
            )?;
            let text = format!(
                "Created project {} (id {}). Ingest key: {}\nNext: get_integration_snippets project={}",
                p["slug"].as_str().unwrap_or_default(),
                p["id"],
                p["ingest_key"].as_str().unwrap_or_default(),
                p["slug"].as_str().unwrap_or_default(),
            );
            Ok((p, text))
        }
        "update_project" => {
            let id = project_id(ctx, &a.req_str("project")?).await?;
            let p = to_value(
                admin::op_update_project(state, id, project_input(a)?)
                    .await
                    .map_err(app_err)?,
            )?;
            Ok((p.clone(), format!("Updated project.\n{}", project_line(&p))))
        }
        "rotate_project_key" => {
            let id = project_id(ctx, &a.req_str("project")?).await?;
            let p = to_value(admin::op_rotate_key(state, id).await.map_err(app_err)?)?;
            let text = format!(
                "Rotated. New ingest key for {}: {} (the old key no longer works; update every client).",
                p["slug"].as_str().unwrap_or_default(),
                p["ingest_key"].as_str().unwrap_or_default(),
            );
            Ok((p, text))
        }
        "delete_project" => {
            let reference = a.req_str("project")?;
            let id = project_id(ctx, &reference).await?;
            let p = admin::op_get_project(state, id).await.map_err(app_err)?;
            let confirm = a.str("confirm_slug")?.unwrap_or_default();
            if confirm.trim() != p.project.slug {
                return Err(format!(
                    "confirmation_required: confirm_slug must be exactly {:?}. This deletes the project and its {} feedback item(s) permanently.",
                    p.project.slug,
                    p.stats.total + p.stats.spam
                ));
            }
            admin::op_delete_project(state, id).await.map_err(app_err)?;
            Ok((
                json!({ "deleted": true, "id": id, "slug": p.project.slug }),
                format!("Deleted project {} and all its feedback.", p.project.slug),
            ))
        }
        "create_feedback" => {
            let id = project_id(ctx, &a.req_str("project")?).await?;
            let meta = match a.0.get("meta") {
                None | Some(Value::Null) => None,
                Some(v @ Value::Object(_)) => Some(v.clone()),
                Some(_) => return Err("invalid_arguments: meta must be an object".into()),
            };
            let input = AdminFeedbackInput {
                kind: a.str("kind")?,
                title: a.str("title")?,
                body: a.str("body")?,
                contact: a.str("contact")?,
                rating: a.int("rating")?,
                meta,
                status: a.str("status")?,
                admin_note: a.str("admin_note")?,
                created_at: a.time("created_at", false)?,
            };
            let fid = public::op_create_feedback(state, id, input)
                .await
                .map_err(app_err)?;
            let v = admin::op_get_feedback(state, fid).await.map_err(app_err)?;
            let item = v["item"].clone();
            Ok((
                item.clone(),
                format!("Created feedback #{fid}.\n{}", pretty(&item)),
            ))
        }
        "update_feedback" => {
            let id = a.req_int("id")?;
            if a.has("admin_note") && a.has("append_note") {
                return Err("invalid_arguments: pass admin_note or append_note, not both".into());
            }
            let mut patch = FeedbackPatch {
                status: a.str("status")?,
                admin_note: a.str("admin_note")?,
                kind: a.str("kind")?,
            };
            if let Some(extra) = a.str("append_note")?.filter(|s| !s.trim().is_empty()) {
                if extra.chars().count() > 2000 {
                    return Err(
                        "invalid_arguments: append_note must be at most 2000 characters".into(),
                    );
                }
                let current = admin::op_get_feedback(state, id).await.map_err(app_err)?;
                let existing = current["item"]["admin_note"].as_str().unwrap_or_default();
                let line = format!("[{}] {}", date(now()), extra.trim());
                patch.admin_note = Some(if existing.trim().is_empty() {
                    line
                } else {
                    format!("{}\n{line}", existing.trim_end())
                });
            }
            if patch.status.is_none() && patch.admin_note.is_none() && patch.kind.is_none() {
                return Err("invalid_arguments: nothing to update (status, kind, admin_note or append_note)".into());
            }
            let f = to_value(admin::op_update_feedback(state, id, patch).await.map_err(
                |e| match e {
                    AppError::NotFound => format!("not_found: no feedback with id {id}"),
                    e => app_err(e),
                },
            )?)?;
            let slugs = slug_map(ctx).await?;
            Ok((
                f.clone(),
                format!("Updated.\n{}", feedback_line(&f, &slugs)),
            ))
        }
        "bulk_update_feedback" => {
            let ids: Vec<i64> = match a.0.get("ids") {
                Some(Value::Array(v)) => v
                    .iter()
                    .map(|x| x.as_i64().ok_or("invalid_arguments: ids must be integers"))
                    .collect::<Result<_, _>>()?,
                _ => return Err("invalid_arguments: ids must be an array of integers".into()),
            };
            let status = a.req_str("status")?;
            let v = admin::op_bulk(
                state,
                BulkBody {
                    ids,
                    action: "set_status".into(),
                    status: Some(status.clone()),
                },
            )
            .await
            .map_err(app_err)?;
            let text = format!("Set status {status} on {} item(s).", v["affected"]);
            Ok((v, text))
        }
        "delete_feedback" => {
            let id = a.req_int("id")?;
            if a.bool("confirm")? != Some(true) {
                return Err("confirmation_required: pass confirm=true to permanently delete. Consider status archived or spam instead.".into());
            }
            admin::op_delete_feedback(state, id)
                .await
                .map_err(|e| match e {
                    AppError::NotFound => format!("not_found: no feedback with id {id}"),
                    e => app_err(e),
                })?;
            Ok((
                json!({ "deleted": true, "id": id }),
                format!("Deleted feedback #{id}."),
            ))
        }
        "reindex_embeddings" => {
            if !state.embedder.enabled() {
                return Err("unavailable: embeddings are disabled (FH_EMBED_PROVIDER=none)".into());
            }
            admin::op_reindex(state).await.map_err(app_err)?;
            Ok((
                json!({ "ok": true, "model": state.embedder.model_id() }),
                format!(
                    "Re-embedding everything with {} in the background. Watch get_stats embeddings.pending.",
                    state.embedder.model_id()
                ),
            ))
        }
        _ => Err(format!("unknown tool {name}")),
    }
}

fn project_input(a: &Args<'_>) -> Result<ProjectInput, String> {
    Ok(ProjectInput {
        title: a.str("title")?,
        slug: a.str("slug")?,
        project_type: a.str("project_type")?,
        description: a.str("description")?,
        accepting: a.bool("accepting")?,
    })
}

async fn find_similar(ctx: &Ctx, a: &Args<'_>) -> ToolResult {
    let state = &ctx.state;
    let limit = a.int("limit")?.unwrap_or(10).clamp(1, 50) as usize;
    let id = a.int("id")?;
    let text = a.str("text")?.filter(|t| !t.trim().is_empty());
    let mut project = opt_project(ctx, a).await?;

    let query_text = match (id, &text) {
        (Some(_), Some(_)) => return Err("invalid_arguments: pass id or text, not both".into()),
        (None, None) => return Err("invalid_arguments: pass id or text".into()),
        (Some(id), None) => {
            let v = admin::op_get_feedback(state, id)
                .await
                .map_err(|e| match e {
                    AppError::NotFound => format!("not_found: no feedback with id {id}"),
                    e => app_err(e),
                })?;
            let item = &v["item"];
            project = project.or(item["project_id"].as_i64());
            embed::feedback_text(
                item["kind"].as_str().unwrap_or_default(),
                item["title"].as_str().unwrap_or_default(),
                item["body"].as_str().unwrap_or_default(),
            )
        }
        (None, Some(t)) => t.clone(),
    };

    let vector = match id.and_then(|i| state.index.get(i)) {
        Some(v) => Some(v),
        None if state.embedder.enabled() => state
            .embedder
            .embed(std::slice::from_ref(&query_text))
            .await
            .ok()
            .and_then(|mut v| (!v.is_empty()).then(|| v.swap_remove(0))),
        None => None,
    };

    let (items, semantic) = match vector {
        Some(v) => {
            let items = admin::op_similar_to_vector(state, &v, project, id, limit)
                .await
                .map_err(app_err)?;
            (to_value(items)?, true)
        }
        None => {
            // Lexical fallback: hybrid search over the text itself.
            let mut q = HashMap::new();
            q.insert("q".to_string(), snippet(&query_text, 300));
            q.insert("status".to_string(), "all".to_string());
            q.insert("limit".to_string(), (limit + 1).to_string());
            if let Some(p) = project {
                q.insert("project_id".to_string(), p.to_string());
            }
            let page = admin::op_list_feedback(state, &q).await.map_err(app_err)?;
            let items: Vec<Value> = page["items"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|f| f["id"].as_i64() != id)
                .take(limit)
                .collect();
            (Value::Array(items), false)
        }
    };
    let slugs = slug_map(ctx).await?;
    let list = items.as_array().cloned().unwrap_or_default();
    let mut text = format!(
        "{} similar item(s){}\n",
        list.len(),
        if semantic {
            ""
        } else {
            " (lexical fallback, no vectors)"
        }
    );
    for f in &list {
        text.push_str(&feedback_line(f, &slugs));
        text.push('\n');
    }
    Ok((json!({ "items": items, "semantic": semantic }), text))
}

async fn snippets(ctx: &Ctx, a: &Args<'_>) -> ToolResult {
    let id = project_id(ctx, &a.req_str("project")?).await?;
    let p = admin::op_get_project(&ctx.state, id)
        .await
        .map_err(app_err)?;
    let base = a
        .str("base_url")?
        .map(|b| b.trim().trim_end_matches('/').to_string())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| ctx.base_url.clone());
    let key = &p.project.ingest_key;
    let slug = &p.project.slug;
    let hosted = format!("{base}/s/{slug}");
    let floating =
        format!("<script src=\"{base}/widget.js\" data-fh-key=\"{key}\" defer></script>");
    let inline = format!(
        "<div data-fh-inline data-fh-key=\"{key}\" data-fh-project=\"{}\"></div>\n<script src=\"{base}/widget.js\" defer></script>",
        crate::routes::html_escape(&p.project.title)
    );
    let esm = format!(
        "import {{ createClient }} from '@feedback-harvest/widget';\n\nconst fh = createClient({{ endpoint: '{base}', key: '{key}' }});\nfh.prepare(); // start solving the challenge when the dialog opens\nawait fh.submit({{ kind: 'bug', title: 'Crash on save', body: 'Steps...', meta: {{ version: '1.2.3' }} }});"
    );
    let curl = format!(
        "CH=$(curl -s '{base}/api/v1/challenge?key={key}')\nTOKEN=$(echo \"$CH\" | jq -r .challenge); BITS=$(echo \"$CH\" | jq -r .bits)\nNONCE=$(feedback-harvest solve \"$TOKEN\" \"$BITS\")\ncurl -s -X POST '{base}/api/v1/feedback' -H 'content-type: application/json' \\\n  -d \"{{\\\"key\\\":\\\"{key}\\\",\\\"kind\\\":\\\"bug\\\",\\\"body\\\":\\\"It crashed\\\",\\\"challenge\\\":\\\"$TOKEN\\\",\\\"nonce\\\":\\\"$NONCE\\\"}}\""
    );
    let structured = json!({
        "project": slug,
        "ingest_key": key,
        "accepting": p.project.accepting,
        "hosted_url": hosted,
        "widget_floating": floating,
        "widget_inline": inline,
        "esm_client": esm,
        "curl": curl,
        "protocol_docs": "docs/api.md and docs/integrations.md in the feedback-harvest repo",
    });
    let mut text = format!(
        "Integration for {slug} (ingest key {key}, public by design, rotate if abused){}\n\n",
        if p.project.accepting {
            ""
        } else {
            "\nNOTE: project is paused, submissions are rejected until accepting=true."
        }
    );
    text.push_str(&format!("Hosted page:\n{hosted}\n\nFloating widget (any website):\n{floating}\n\nInline form:\n{inline}\n\nESM client:\n{esm}\n\ncurl (any language, see docs/integrations.md):\n{curl}\n"));
    Ok((structured, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_times() {
        assert_eq!(parse_time("1700000000", false), Some(1_700_000_000));
        assert_eq!(parse_time("2026-10-01", false), Some(1_790_812_800));
        assert_eq!(parse_time("2026-10-01", true), Some(1_790_812_800 + 86399));
        assert_eq!(
            parse_time("2026-10-01T00:00:10Z", false),
            Some(1_790_812_810)
        );
        assert_eq!(parse_time("yesterday", false), None);
    }

    #[test]
    fn every_tool_has_object_schema_and_annotations() {
        for t in list(Scope::Write) {
            assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
            assert!(t["annotations"]["readOnlyHint"].is_boolean());
            assert!(t["description"].as_str().unwrap().len() > 40);
        }
        let read = list(Scope::Read);
        assert!(
            read.iter()
                .all(|t| t["annotations"]["readOnlyHint"] == true)
        );
    }
}
