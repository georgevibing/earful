//! SQLite access: connection setup, embedded migrations and the async wrapper.

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

/// Migrations are append-only. Each entry runs once, in order, inside a transaction.
const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE projects (
    id           INTEGER PRIMARY KEY,
    slug         TEXT NOT NULL UNIQUE,
    title        TEXT NOT NULL,
    project_type TEXT NOT NULL DEFAULT 'other',
    description  TEXT NOT NULL DEFAULT '',
    ingest_key   TEXT NOT NULL UNIQUE,
    accepting    INTEGER NOT NULL DEFAULT 1,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE TABLE feedback (
    id           INTEGER PRIMARY KEY,
    project_id   INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('bug', 'feedback')),
    title        TEXT NOT NULL,
    body         TEXT NOT NULL,
    contact      TEXT,
    rating       INTEGER CHECK (rating BETWEEN 1 AND 5),
    meta         TEXT NOT NULL DEFAULT '{}',
    source       TEXT NOT NULL DEFAULT 'api',
    status       TEXT NOT NULL DEFAULT 'new'
                 CHECK (status IN ('new', 'triaged', 'resolved', 'archived', 'spam')),
    spam_reason  TEXT,
    spam_score   REAL NOT NULL DEFAULT 0,
    admin_note   TEXT NOT NULL DEFAULT '',
    content_hash TEXT NOT NULL,
    dup_count    INTEGER NOT NULL DEFAULT 0,
    ip_hash      TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL
);
CREATE INDEX feedback_project_created ON feedback (project_id, created_at DESC);
CREATE INDEX feedback_status ON feedback (status);
CREATE INDEX feedback_hash ON feedback (project_id, content_hash, created_at);
CREATE INDEX feedback_ip ON feedback (ip_hash, created_at);

-- Word index (stemmed) for keyword search with BM25 ranking.
CREATE VIRTUAL TABLE feedback_fts USING fts5 (
    title, body, contact,
    content = 'feedback', content_rowid = 'id',
    tokenize = 'porter unicode61 remove_diacritics 2'
);
-- Trigram index for substring and typo tolerant fuzzy find.
CREATE VIRTUAL TABLE feedback_tri USING fts5 (
    title, body,
    content = 'feedback', content_rowid = 'id',
    tokenize = 'trigram'
);

CREATE TRIGGER feedback_ai AFTER INSERT ON feedback BEGIN
    INSERT INTO feedback_fts (rowid, title, body, contact) VALUES (new.id, new.title, new.body, new.contact);
    INSERT INTO feedback_tri (rowid, title, body) VALUES (new.id, new.title, new.body);
END;
CREATE TRIGGER feedback_ad AFTER DELETE ON feedback BEGIN
    INSERT INTO feedback_fts (feedback_fts, rowid, title, body, contact) VALUES ('delete', old.id, old.title, old.body, old.contact);
    INSERT INTO feedback_tri (feedback_tri, rowid, title, body) VALUES ('delete', old.id, old.title, old.body);
END;
CREATE TRIGGER feedback_au AFTER UPDATE OF title, body, contact ON feedback BEGIN
    INSERT INTO feedback_fts (feedback_fts, rowid, title, body, contact) VALUES ('delete', old.id, old.title, old.body, old.contact);
    INSERT INTO feedback_tri (feedback_tri, rowid, title, body) VALUES ('delete', old.id, old.title, old.body);
    INSERT INTO feedback_fts (rowid, title, body, contact) VALUES (new.id, new.title, new.body, new.contact);
    INSERT INTO feedback_tri (rowid, title, body) VALUES (new.id, new.title, new.body);
END;

CREATE TABLE embeddings (
    feedback_id INTEGER PRIMARY KEY REFERENCES feedback(id) ON DELETE CASCADE,
    model       TEXT NOT NULL,
    vec         BLOB NOT NULL
);

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

-- Sliding window counters. `bucket` is e.g. 'submit:<project>', 'challenge', 'login'.
CREATE TABLE rate_events (
    bucket  TEXT NOT NULL,
    ip_hash TEXT NOT NULL,
    ts      INTEGER NOT NULL
);
CREATE INDEX rate_events_lookup ON rate_events (bucket, ip_hash, ts);
CREATE INDEX rate_events_ts ON rate_events (ts);

CREATE TABLE used_challenges (
    id         TEXT PRIMARY KEY,
    expires_at INTEGER NOT NULL
);

-- Rejected or quarantined submissions, for the dashboard.
CREATE TABLE block_events (
    project_id INTEGER,
    reason     TEXT NOT NULL,
    ts         INTEGER NOT NULL
);
CREATE INDEX block_events_ts ON block_events (ts);
"#,
    r#"
-- Bearer tokens for MCP clients and scripts. Only the SHA-256 of the token is stored.
CREATE TABLE api_tokens (
    id           INTEGER PRIMARY KEY,
    name         TEXT NOT NULL,
    scope        TEXT NOT NULL CHECK (scope IN ('read', 'write')),
    token_hash   TEXT NOT NULL UNIQUE,
    prefix       TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    last_used_at INTEGER
);
"#,
    r#"
-- Blocked submissions as daily counters instead of one row per request, so a
-- flood of rejected requests cannot grow the database without bound.
CREATE TABLE block_counts (
    day        INTEGER NOT NULL, -- Unix day (ts / 86400, UTC)
    project_id INTEGER NOT NULL, -- 0 when unknown
    reason     TEXT NOT NULL,
    n          INTEGER NOT NULL,
    PRIMARY KEY (day, project_id, reason)
);
INSERT INTO block_counts (day, project_id, reason, n)
    SELECT ts / 86400, COALESCE(project_id, 0), reason, COUNT(*) FROM block_events GROUP BY 1, 2, 3;
DROP TABLE block_events;

-- The near duplicate check runs once per item, so a reindex or model change
-- never overrides an admin decision. Already embedded rows count as checked.
ALTER TABLE feedback ADD COLUMN dup_checked INTEGER NOT NULL DEFAULT 0;
UPDATE feedback SET dup_checked = 1 WHERE id IN (SELECT feedback_id FROM embeddings);
"#,
    r#"
-- Project-scoped themes, with composite foreign keys enforcing assignment scope.
CREATE UNIQUE INDEX feedback_id_project ON feedback(id, project_id);
CREATE TABLE labels (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    name_key TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT 'slate',
    description TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id, name_key),
    UNIQUE(id, project_id)
);
CREATE TABLE feedback_labels (
    feedback_id INTEGER NOT NULL,
    label_id INTEGER NOT NULL,
    project_id INTEGER NOT NULL,
    PRIMARY KEY(feedback_id, label_id),
    FOREIGN KEY(feedback_id, project_id) REFERENCES feedback(id, project_id) ON DELETE CASCADE,
    FOREIGN KEY(label_id, project_id) REFERENCES labels(id, project_id) ON DELETE CASCADE
);
CREATE INDEX feedback_labels_label ON feedback_labels(label_id, feedback_id);
"#,
];

pub fn open(path: &Path) -> Result<Connection> {
    let conn =
        Connection::open(path).with_context(|| format!("opening database {}", path.display()))?;
    configure(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    Ok(())
}

pub fn migrate(conn: &mut Connection) -> Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (idx, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)
            .with_context(|| format!("running migration {}", idx + 1))?;
        tx.pragma_update(None, "user_version", (idx + 1) as i64)?;
        tx.commit()?;
        tracing::info!(version = idx + 1, "applied migration");
    }
    Ok(())
}

/// Returns the server secret used for HMACs, creating it on first boot.
pub fn server_secret(conn: &Connection) -> Result<Vec<u8>> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'server_secret'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(hex_secret) = existing {
        return Ok(hex::decode(hex_secret)?);
    }
    let mut secret = [0u8; 32];
    rand::fill(&mut secret);
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('server_secret', ?1)",
        params![hex::encode(secret)],
    )?;
    Ok(secret.to_vec())
}

/// Cheap cloneable handle. SQLite serialises writes anyway, so a single
/// connection behind a mutex is plenty for a single-admin tool.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    /// Runs a closure against the connection on the blocking thread pool.
    pub async fn call<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            f(&mut guard)
        })
        .await
        .context("database task panicked")?
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}
