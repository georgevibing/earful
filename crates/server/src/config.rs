//! Runtime configuration, read once from `FH_*` environment variables.

use std::{net::SocketAddr, path::PathBuf, str::FromStr};

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Config {
    /// Listen address. Defaults to loopback; the Docker image overrides it.
    pub bind: SocketAddr,
    pub database_path: PathBuf,
    /// Argon2 PHC string. When `None`, every admin login is rejected.
    pub admin_password_hash: Option<String>,
    pub session_ttl_hours: i64,
    pub cookie_secure: bool,
    /// Number of reverse proxies in front of us whose `X-Forwarded-For` entries we trust.
    pub trusted_proxy_hops: usize,
    pub limits: Limits,
    pub pow: PowConfig,
    pub embed: EmbedConfig,
}

#[derive(Debug, Clone)]
pub struct Limits {
    pub per_ip_project_minute: u32,
    pub per_ip_project_hour: u32,
    pub per_ip_project_day: u32,
    pub per_ip_global_day: u32,
    pub per_project_hour: u32,
    pub challenges_per_ip_hour: u32,
    pub logins_per_ip_window: u32,
    pub login_window_secs: i64,
    /// Submissions arriving faster than this after the challenge was issued are quarantined.
    pub min_fill_secs: i64,
    /// Heuristic score at or above which a submission lands in the spam bucket.
    pub spam_threshold: f32,
}

#[derive(Debug, Clone)]
pub struct PowConfig {
    pub base_bits: u32,
    pub max_bits: u32,
    pub challenge_ttl_secs: i64,
}

#[derive(Debug, Clone)]
pub enum EmbedConfig {
    Disabled,
    /// Built-in feature-hashing embedder. Offline and deterministic, lexical rather than truly semantic.
    Hash {
        dim: usize,
    },
    /// Any OpenAI-compatible `/embeddings` endpoint (OpenAI, Ollama, LM Studio, vLLM, ...).
    OpenAi {
        url: String,
        model: String,
        api_key: Option<String>,
    },
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            per_ip_project_minute: 3,
            per_ip_project_hour: 10,
            per_ip_project_day: 30,
            per_ip_global_day: 60,
            per_project_hour: 300,
            challenges_per_ip_hour: 60,
            logins_per_ip_window: 5,
            login_window_secs: 15 * 60,
            min_fill_secs: 2,
            spam_threshold: 0.7,
        }
    }
}

impl Default for PowConfig {
    fn default() -> Self {
        Self {
            base_bits: 18,
            max_bits: 23,
            challenge_ttl_secs: 30 * 60,
        }
    }
}

impl Config {
    /// Sensible defaults for tests and local development.
    pub fn for_tests(database_path: PathBuf) -> Self {
        Self {
            bind: "127.0.0.1:0".parse().unwrap(),
            database_path,
            admin_password_hash: None,
            session_ttl_hours: 24,
            cookie_secure: false,
            trusted_proxy_hops: 1,
            limits: Limits {
                min_fill_secs: 0,
                ..Limits::default()
            },
            pow: PowConfig {
                base_bits: 4,
                max_bits: 8,
                challenge_ttl_secs: 600,
            },
            embed: EmbedConfig::Hash { dim: 384 },
        }
    }

    pub fn from_env() -> Result<Self> {
        let limits_default = Limits::default();
        let pow_default = PowConfig::default();

        let admin_password_hash = match env("FH_ADMIN_PASSWORD_HASH_FILE") {
            Some(path) => Some(
                std::fs::read_to_string(&path)
                    .with_context(|| format!("reading FH_ADMIN_PASSWORD_HASH_FILE {path}"))?
                    .trim()
                    .to_string(),
            ),
            None => env("FH_ADMIN_PASSWORD_HASH"),
        }
        .filter(|s| !s.is_empty());

        let embed = match env("FH_EMBED_PROVIDER").as_deref().unwrap_or("hash") {
            "none" | "disabled" => EmbedConfig::Disabled,
            "hash" => EmbedConfig::Hash {
                dim: parse_or("FH_EMBED_DIM", 384)?,
            },
            "openai" => EmbedConfig::OpenAi {
                url: env("FH_EMBED_URL").unwrap_or_else(|| "https://api.openai.com/v1".into()),
                model: env("FH_EMBED_MODEL").unwrap_or_else(|| "text-embedding-3-small".into()),
                api_key: match env("FH_EMBED_API_KEY_FILE") {
                    Some(path) => Some(std::fs::read_to_string(&path)?.trim().to_string()),
                    None => env("FH_EMBED_API_KEY"),
                },
            },
            other => {
                anyhow::bail!("unknown FH_EMBED_PROVIDER {other:?} (use none, hash or openai)")
            }
        };

        Ok(Self {
            bind: parse_or("FH_BIND", "127.0.0.1:8080".parse()?)?,
            database_path: env("FH_DATABASE_PATH")
                .unwrap_or_else(|| "feedback-harvest.db".into())
                .into(),
            admin_password_hash,
            session_ttl_hours: parse_or("FH_SESSION_TTL_HOURS", 24 * 7)?,
            cookie_secure: parse_or("FH_COOKIE_SECURE", true)?,
            trusted_proxy_hops: parse_or("FH_TRUSTED_PROXY_HOPS", 0)?,
            limits: Limits {
                per_ip_project_minute: parse_or(
                    "FH_LIMIT_IP_PROJECT_MINUTE",
                    limits_default.per_ip_project_minute,
                )?,
                per_ip_project_hour: parse_or(
                    "FH_LIMIT_IP_PROJECT_HOUR",
                    limits_default.per_ip_project_hour,
                )?,
                per_ip_project_day: parse_or(
                    "FH_LIMIT_IP_PROJECT_DAY",
                    limits_default.per_ip_project_day,
                )?,
                per_ip_global_day: parse_or(
                    "FH_LIMIT_IP_GLOBAL_DAY",
                    limits_default.per_ip_global_day,
                )?,
                per_project_hour: parse_or(
                    "FH_LIMIT_PROJECT_HOUR",
                    limits_default.per_project_hour,
                )?,
                challenges_per_ip_hour: parse_or(
                    "FH_LIMIT_CHALLENGES_IP_HOUR",
                    limits_default.challenges_per_ip_hour,
                )?,
                logins_per_ip_window: limits_default.logins_per_ip_window,
                login_window_secs: limits_default.login_window_secs,
                min_fill_secs: parse_or("FH_MIN_FILL_SECS", limits_default.min_fill_secs)?,
                spam_threshold: parse_or("FH_SPAM_THRESHOLD", limits_default.spam_threshold)?,
            },
            pow: PowConfig {
                base_bits: parse_or("FH_POW_BITS", pow_default.base_bits)?,
                max_bits: parse_or("FH_POW_MAX_BITS", pow_default.max_bits)?,
                challenge_ttl_secs: parse_or(
                    "FH_CHALLENGE_TTL_SECS",
                    pow_default.challenge_ttl_secs,
                )?,
            },
            embed,
        })
    }
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string())
}

fn parse_or<T>(key: &str, default: T) -> Result<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    match env(key) {
        Some(v) if !v.is_empty() => v
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid value for {key}: {e}")),
        _ => Ok(default),
    }
}
