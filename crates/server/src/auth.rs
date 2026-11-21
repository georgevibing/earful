//! Single-admin authentication: argon2id password check and server side sessions.

use std::sync::LazyLock;

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, Method, header, request::Parts},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use rusqlite::{OptionalExtension, params};

use crate::{
    crypto,
    db::now,
    error::{AppError, AppResult},
    state::AppState,
};

/// Session cookie name over plain http (local development).
pub const SESSION_COOKIE: &str = "earful_session";
/// Session cookie name when cookies are `Secure`. The `__Host-` prefix makes
/// browsers refuse it unless it is host-only, so sibling subdomains cannot set
/// or overwrite it (cookie tossing).
pub const SESSION_COOKIE_HOST: &str = "__Host-earful_session";
/// Long-lived marker for browsers that have signed in before. It lets them
/// bypass the global login cap, so strangers cannot lock the admin out by
/// spraying wrong passwords from many IPs (OWASP "device cookie" pattern).
const DEVICE_COOKIE: &str = "earful_device";
const DEVICE_COOKIE_HOST: &str = "__Host-earful_device";
const DEVICE_TTL_SECS: i64 = 365 * 86400;

pub fn session_cookie_name(state: &AppState) -> &'static str {
    if state.cfg.cookie_secure {
        SESSION_COOKIE_HOST
    } else {
        SESSION_COOKIE
    }
}

fn device_cookie_name(state: &AppState) -> &'static str {
    if state.cfg.cookie_secure {
        DEVICE_COOKIE_HOST
    } else {
        DEVICE_COOKIE
    }
}

/// Raw session token from either cookie name.
pub fn session_token(jar: &CookieJar) -> Option<String> {
    jar.get(SESSION_COOKIE_HOST)
        .or_else(|| jar.get(SESSION_COOKIE))
        .map(|c| c.value().to_string())
}

/// Fingerprint of the configured admin password hash. Sessions remember it, so
/// changing the password signs out every existing session.
pub fn password_fingerprint(state: &AppState) -> String {
    let hash = state.cfg.admin_password_hash.as_deref().unwrap_or("");
    crypto::hmac_hex(&state.secret, format!("pw:{hash}").as_bytes())[..32].to_string()
}

/// Bound to the password fingerprint, so changing the password also revokes
/// every device's exemption from the global login cap.
fn device_data(state: &AppState, id: &str) -> String {
    format!("device:{id}:{}", password_fingerprint(state))
}

fn device_mac(state: &AppState, id: &str) -> String {
    crypto::hmac_hex(&state.secret, device_data(state, id).as_bytes())[..32].to_string()
}

pub fn device_cookie(state: &AppState) -> Cookie<'static> {
    let id = crypto::random_hex(16);
    let value = format!("{id}.{}", device_mac(state, &id));
    Cookie::build((device_cookie_name(state), value))
        .path("/")
        .http_only(true)
        .secure(state.cfg.cookie_secure)
        .same_site(SameSite::Strict)
        .max_age(time_duration(DEVICE_TTL_SECS))
        .build()
}

/// True when the request carries a device cookie this server issued.
pub fn known_device(state: &AppState, jar: &CookieJar) -> bool {
    let Some(value) = jar
        .get(DEVICE_COOKIE_HOST)
        .or_else(|| jar.get(DEVICE_COOKIE))
        .map(|c| c.value().to_string())
    else {
        return false;
    };
    let Some((id, mac)) = value.split_once('.') else {
        return false;
    };
    id.len() == 32
        && crypto::hmac_verify_hex(&state.secret, device_data(state, id).as_bytes(), mac, 16)
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("hashing password: {e}"))
}

/// Constant-ish time: always runs argon2, even when no admin hash is configured.
pub fn verify_password(configured: Option<&str>, password: &str) -> bool {
    // A throwaway hash so the "not configured" path costs the same as a real check.
    static DUMMY: LazyLock<String> =
        LazyLock::new(|| hash_password("unconfigured").unwrap_or_default());
    let (hash, real) = match configured {
        Some(h) => (h, true),
        None => (DUMMY.as_str(), false),
    };
    let Ok(parsed) = PasswordHash::new(hash) else {
        tracing::error!("EARFUL_ADMIN_PASSWORD_HASH is not a valid argon2 PHC string");
        return false;
    };
    let ok = Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok();
    ok && real
}

pub fn session_cookie(state: &AppState, token: String) -> Cookie<'static> {
    Cookie::build((session_cookie_name(state), token))
        .path("/")
        .http_only(true)
        .secure(state.cfg.cookie_secure)
        .same_site(SameSite::Strict)
        .max_age(time_duration(state.cfg.session_ttl_hours * 3600))
        .build()
}

pub fn clear_cookie(state: &AppState) -> Cookie<'static> {
    Cookie::build((session_cookie_name(state), ""))
        .path("/")
        .http_only(true)
        .secure(state.cfg.cookie_secure)
        .same_site(SameSite::Strict)
        .max_age(time_duration(0))
        .build()
}

fn time_duration(secs: i64) -> time::Duration {
    time::Duration::seconds(secs)
}

/// Creates a session and returns the raw token (only its hash is stored).
pub async fn create_session(state: &AppState) -> AppResult<String> {
    let token = crypto::random_hex(32);
    let token_hash = crypto::sha256_hex(token.as_bytes());
    let ts = now();
    let expires = ts + state.cfg.session_ttl_hours * 3600;
    let fp = password_fingerprint(state);
    state
        .db
        .call(move |c| {
            c.execute(
                "INSERT INTO sessions (token_hash, created_at, expires_at, pw_fp) VALUES (?1, ?2, ?3, ?4)",
                params![token_hash, ts, expires, fp],
            )?;
            Ok(())
        })
        .await?;
    Ok(token)
}

pub async fn destroy_session(state: &AppState, token: &str) -> AppResult<()> {
    let token_hash = crypto::sha256_hex(token.as_bytes());
    state
        .db
        .call(move |c| {
            c.execute(
                "DELETE FROM sessions WHERE token_hash = ?1",
                params![token_hash],
            )?;
            Ok(())
        })
        .await?;
    Ok(())
}

/// Signs out every session (all devices).
pub async fn destroy_all_sessions(state: &AppState) -> AppResult<()> {
    state
        .db
        .call(|c| {
            c.execute("DELETE FROM sessions", [])?;
            Ok(())
        })
        .await?;
    Ok(())
}

/// Extractor that only succeeds for requests carrying a valid admin session.
/// State changing requests must also come from our own origin (CSRF defence in
/// depth on top of `SameSite=Strict`).
pub struct Admin;

impl FromRequestParts<AppState> for Admin {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if parts.method != Method::GET
            && parts.method != Method::HEAD
            && !same_origin(&parts.headers)
        {
            return Err(AppError::Forbidden(
                "bad_origin",
                "cross origin request rejected".into(),
            ));
        }
        let jar = CookieJar::from_headers(&parts.headers);
        let Some(token) = session_token(&jar) else {
            return Err(AppError::Unauthorized);
        };
        if token.len() != 64 {
            return Err(AppError::Unauthorized);
        }
        let token_hash = crypto::sha256_hex(token.as_bytes());
        let ts = now();
        let fp = password_fingerprint(state);
        let valid = state
            .db
            .call(move |c| {
                Ok(c.query_row(
                    "SELECT 1 FROM sessions WHERE token_hash = ?1 AND expires_at > ?2 AND pw_fp = ?3",
                    params![token_hash, ts, fp],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
            })
            .await?;
        if valid {
            Ok(Admin)
        } else {
            Err(AppError::Unauthorized)
        }
    }
}

/// Accepts requests without an Origin header (non-browser clients cannot carry
/// our SameSite=Strict cookie cross-site anyway) or whose Origin host matches Host.
pub fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    let origin_host = origin
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(origin);
    host.is_some_and(|h| h.eq_ignore_ascii_case(origin_host))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trip() {
        let hash = hash_password("correct horse").unwrap();
        assert!(verify_password(Some(&hash), "correct horse"));
        assert!(!verify_password(Some(&hash), "wrong"));
    }

    #[test]
    fn unconfigured_hash_fails_closed() {
        assert!(!verify_password(None, ""));
        assert!(!verify_password(None, "anything"));
        assert!(!verify_password(Some("not-a-phc-string"), "x"));
    }
}
