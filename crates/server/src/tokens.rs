//! Admin-issued API tokens for MCP clients and scripts.
//!
//! Format `fht_` + 64 hex chars. Only `sha256(token)` is stored, so a leaked
//! database does not leak usable tokens. Lookups go by hash, which keeps the
//! verification cost independent of how close a guess is.

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::{
    crypto,
    db::now,
    error::{AppError, AppResult},
    ratelimit::{self, Rule},
    state::AppState,
};

pub const PREFIX: &str = "fht_";
const MAX_NAME: usize = 100;
/// Failed bearer attempts allowed per IP per window before answering 429.
pub const FAILED_AUTH_LIMIT: u32 = 20;
pub const FAILED_AUTH_WINDOW_SECS: i64 = 15 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Read,
    Write,
}

impl Scope {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }

    pub fn can_write(self) -> bool {
        self == Self::Write
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Token {
    pub id: i64,
    pub name: String,
    pub scope: Scope,
    pub prefix: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

fn from_row(r: &rusqlite::Row) -> rusqlite::Result<Token> {
    let scope: String = r.get(2)?;
    Ok(Token {
        id: r.get(0)?,
        name: r.get(1)?,
        scope: Scope::parse(&scope).unwrap_or(Scope::Read),
        prefix: r.get(3)?,
        created_at: r.get(4)?,
        last_used_at: r.get(5)?,
    })
}

const COLUMNS: &str = "id, name, scope, prefix, created_at, last_used_at";

pub fn list(c: &Connection) -> rusqlite::Result<Vec<Token>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {COLUMNS} FROM api_tokens ORDER BY created_at DESC, id DESC"
    ))?;
    stmt.query_map([], from_row)?.collect()
}

pub fn validate_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(AppError::bad(
            "invalid_name",
            format!("name must be 1 to {MAX_NAME} characters"),
        ));
    }
    Ok(name.to_string())
}

/// Creates a token and returns `(raw_token, metadata)`. The raw token is never stored.
pub fn create(c: &Connection, name: &str, scope: Scope) -> rusqlite::Result<(String, Token)> {
    let raw = format!("{PREFIX}{}", crypto::random_hex(32));
    let prefix: String = raw.chars().take(8).collect();
    c.execute(
        "INSERT INTO api_tokens (name, scope, token_hash, prefix, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            name,
            scope.as_str(),
            crypto::sha256_hex(raw.as_bytes()),
            prefix,
            now()
        ],
    )?;
    let token = c.query_row(
        &format!("SELECT {COLUMNS} FROM api_tokens WHERE id = ?1"),
        params![c.last_insert_rowid()],
        from_row,
    )?;
    Ok((raw, token))
}

pub fn delete(c: &Connection, id: i64) -> rusqlite::Result<bool> {
    Ok(c.execute("DELETE FROM api_tokens WHERE id = ?1", params![id])? > 0)
}

/// Resolves a raw token to its metadata, touching `last_used_at` at most once a minute.
pub fn verify(c: &Connection, raw: &str) -> rusqlite::Result<Option<Token>> {
    if !raw.starts_with(PREFIX) || raw.len() != PREFIX.len() + 64 {
        return Ok(None);
    }
    let hash = crypto::sha256_hex(raw.as_bytes());
    let token = c
        .query_row(
            &format!("SELECT {COLUMNS} FROM api_tokens WHERE token_hash = ?1"),
            params![hash],
            from_row,
        )
        .optional()?;
    if let Some(t) = &token {
        let ts = now();
        c.execute(
            "UPDATE api_tokens SET last_used_at = ?2
             WHERE id = ?1 AND (last_used_at IS NULL OR last_used_at < ?2 - 60)",
            params![t.id, ts],
        )?;
    }
    Ok(token)
}

/// Why a bearer check failed. Mapped to HTTP by the caller.
#[derive(Debug)]
pub enum BearerError {
    Missing,
    Invalid,
    RateLimited(i64),
}

/// Checks an `Authorization: Bearer` header value. Failed attempts are rate
/// limited per IP so tokens cannot be brute forced.
pub async fn authenticate(
    state: &AppState,
    authorization: Option<&str>,
    ip_hash: &str,
) -> AppResult<Result<Token, BearerError>> {
    let raw = authorization
        .and_then(|h| {
            h.strip_prefix("Bearer ")
                .or_else(|| h.strip_prefix("bearer "))
        })
        .map(|t| t.trim().to_string());
    let ip = ip_hash.to_string();
    let result = state
        .db
        .call(move |c| {
            let ts = now();
            let rule = [Rule {
                bucket: "mcp_auth",
                ip_hash: Some(&ip),
                window_secs: FAILED_AUTH_WINDOW_SECS,
                limit: FAILED_AUTH_LIMIT,
            }];
            if let Err(retry) = ratelimit::check(c, ts, &rule)? {
                return Ok(Err(BearerError::RateLimited(retry)));
            }
            let Some(raw) = raw else {
                return Ok(Err(BearerError::Missing));
            };
            match verify(c, &raw)? {
                Some(t) => Ok(Ok(t)),
                None => {
                    ratelimit::record(c, "mcp_auth", &ip, ts)?;
                    Ok(Err(BearerError::Invalid))
                }
            }
        })
        .await?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut c).unwrap();
        c
    }

    #[test]
    fn create_verify_delete() {
        let c = conn();
        let (raw, t) = create(&c, "laptop", Scope::Write).unwrap();
        assert!(raw.starts_with("fht_") && raw.len() == 68);
        assert_eq!(t.prefix, &raw[..8]);
        let stored: String = c
            .query_row("SELECT token_hash FROM api_tokens", [], |r| r.get(0))
            .unwrap();
        assert!(!stored.contains(&raw[4..]), "raw token must not be stored");
        let v = verify(&c, &raw).unwrap().unwrap();
        assert_eq!(v.scope, Scope::Write);
        assert!(v.last_used_at.is_none(), "metadata read before touch");
        assert!(verify(&c, &raw).unwrap().unwrap().last_used_at.is_some());
        assert!(verify(&c, "fht_nope").unwrap().is_none());
        assert!(delete(&c, t.id).unwrap());
        assert!(verify(&c, &raw).unwrap().is_none());
    }
}
