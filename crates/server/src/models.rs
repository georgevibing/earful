use rusqlite::Row;
use serde::Serialize;

pub const PROJECT_TYPES: &[&str] = &[
    "game", "webapp", "mobile", "desktop", "service", "library", "other",
];
pub const KINDS: &[&str] = &["bug", "feedback"];
pub const STATUSES: &[&str] = &["new", "triaged", "resolved", "archived", "spam"];

#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub project_type: String,
    pub description: String,
    pub ingest_key: String,
    pub accepting: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Project {
    pub const COLUMNS: &'static str =
        "id, slug, title, project_type, description, ingest_key, accepting, created_at, updated_at";

    pub fn from_row(r: &Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get(0)?,
            slug: r.get(1)?,
            title: r.get(2)?,
            project_type: r.get(3)?,
            description: r.get(4)?,
            ingest_key: r.get(5)?,
            accepting: r.get(6)?,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Feedback {
    pub id: i64,
    pub project_id: i64,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub contact: Option<String>,
    pub rating: Option<i64>,
    pub meta: serde_json::Value,
    pub source: String,
    pub status: String,
    pub spam_reason: Option<String>,
    pub spam_score: f64,
    pub admin_note: String,
    pub dup_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_seen_at: i64,
    /// Relevance score when the row came from a search, otherwise absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    pub labels: Vec<Label>,
}

impl Feedback {
    pub const COLUMNS: &'static str = "id, project_id, kind, title, body, contact, rating, meta, source, status, \
         spam_reason, spam_score, admin_note, dup_count, created_at, updated_at, last_seen_at";

    pub fn from_row(r: &Row) -> rusqlite::Result<Self> {
        let meta: String = r.get(7)?;
        Ok(Self {
            id: r.get(0)?,
            project_id: r.get(1)?,
            kind: r.get(2)?,
            title: r.get(3)?,
            body: r.get(4)?,
            contact: r.get(5)?,
            rating: r.get(6)?,
            meta: serde_json::from_str(&meta).unwrap_or(serde_json::Value::Null),
            source: r.get(8)?,
            status: r.get(9)?,
            spam_reason: r.get(10)?,
            spam_score: r.get(11)?,
            admin_note: r.get(12)?,
            dup_count: r.get(13)?,
            created_at: r.get(14)?,
            updated_at: r.get(15)?,
            last_seen_at: r.get(16)?,
            score: None,
            labels: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Label {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub color: String,
    pub description: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Label {
    pub const COLUMNS: &'static str =
        "l.id, l.project_id, l.name, l.color, l.description, l.created_at, l.updated_at";

    pub fn from_row(r: &Row, offset: usize) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get(offset)?,
            project_id: r.get(offset + 1)?,
            name: r.get(offset + 2)?,
            color: r.get(offset + 3)?,
            description: r.get(offset + 4)?,
            created_at: r.get(offset + 5)?,
            updated_at: r.get(offset + 6)?,
        })
    }
}

/// Lowercase, ascii-only, hyphen separated.
pub fn slugify(input: &str) -> String {
    let mut out = String::new();
    for c in input.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-')
        .chars()
        .take(63)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}

pub fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 63
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !slug.starts_with('-')
        && !slug.ends_with('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify("My Cool Game!"), "my-cool-game");
        assert_eq!(slugify("  --Ünïcode  Thing--"), "n-code-thing");
        assert!(valid_slug("abc-123"));
        assert!(!valid_slug("-abc"));
        assert!(!valid_slug("ABC"));
        assert!(!valid_slug(""));
    }
}
