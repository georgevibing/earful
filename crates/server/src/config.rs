//! Runtime configuration, read once from `EARFUL_*` environment variables.

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    str::FromStr,
};

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
    /// Submissions per day across every project and IP (disk-filling guard).
    pub global_day: u32,
    /// Days quarantined (spam) items are kept before being purged; 0 keeps them.
    pub spam_retention_days: i64,
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
    /// Local Model2Vec static embeddings: real semantic vectors, offline, no GPU.
    Model2Vec {
        /// Directory holding `config.json`, `model.safetensors` and `tokenizer.json`.
        path: PathBuf,
        /// Hugging Face repo id, only used to download a missing model.
        repo: String,
        /// Download the model at startup when `path` is missing.
        auto_download: bool,
    },
}

pub const DEFAULT_MODEL2VEC_REPO: &str = "minishlab/potion-base-8M";
pub const DEFAULT_MODEL2VEC_PATH: &str = "models/potion-base-8M";
pub const MODEL2VEC_FILES: &[&str] = &["config.json", "model.safetensors", "tokenizer.json"];

/// True when `dir` contains every file a Model2Vec model needs.
pub fn model2vec_present(dir: &Path) -> bool {
    MODEL2VEC_FILES.iter().all(|f| dir.join(f).is_file())
}

/// Resolves the embedding provider from `EARFUL_EMBED_*` variables. `lookup` is
/// the environment (injected so tests do not touch the process env).
pub fn embed_config_from(lookup: &dyn Fn(&str) -> Option<String>) -> Result<EmbedConfig> {
    let get = |key: &str| {
        lookup(key)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let dim = match get("EARFUL_EMBED_DIM") {
        Some(v) => v
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid value for EARFUL_EMBED_DIM: {e}"))?,
        None => 384,
    };
    let model2vec = || -> Result<EmbedConfig> {
        Ok(EmbedConfig::Model2Vec {
            path: get("EARFUL_EMBED_MODEL_PATH")
                .unwrap_or_else(|| DEFAULT_MODEL2VEC_PATH.into())
                .into(),
            repo: get("EARFUL_EMBED_MODEL").unwrap_or_else(|| DEFAULT_MODEL2VEC_REPO.into()),
            auto_download: match get("EARFUL_EMBED_AUTO_DOWNLOAD") {
                Some(v) => v.parse().map_err(|e| {
                    anyhow::anyhow!("invalid value for EARFUL_EMBED_AUTO_DOWNLOAD: {e}")
                })?,
                None => false,
            },
        })
    };
    Ok(
        match get("EARFUL_EMBED_PROVIDER").as_deref().unwrap_or("auto") {
            "none" | "disabled" => EmbedConfig::Disabled,
            "hash" => EmbedConfig::Hash { dim },
            "model2vec" => model2vec()?,
            "auto" => match model2vec()? {
                EmbedConfig::Model2Vec { path, .. } if !model2vec_present(&path) => {
                    tracing::info!(
                        path = %path.display(),
                        "no Model2Vec model found, using the lexical hash embedder \
                         (run `earful download-model` for semantic search)"
                    );
                    EmbedConfig::Hash { dim }
                }
                cfg => cfg,
            },
            "openai" => EmbedConfig::OpenAi {
                url: get("EARFUL_EMBED_URL").unwrap_or_else(|| "https://api.openai.com/v1".into()),
                model: get("EARFUL_EMBED_MODEL").unwrap_or_else(|| "text-embedding-3-small".into()),
                api_key: match get("EARFUL_EMBED_API_KEY_FILE") {
                    Some(path) => Some(
                        std::fs::read_to_string(&path)
                            .with_context(|| format!("reading EARFUL_EMBED_API_KEY_FILE {path}"))?
                            .trim()
                            .to_string(),
                    ),
                    None => get("EARFUL_EMBED_API_KEY"),
                },
            },
            other => anyhow::bail!(
                "unknown EARFUL_EMBED_PROVIDER {other:?} (use auto, model2vec, hash, openai or none)"
            ),
        },
    )
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            per_ip_project_minute: 3,
            per_ip_project_hour: 10,
            per_ip_project_day: 30,
            per_ip_global_day: 60,
            per_project_hour: 300,
            global_day: 5000,
            spam_retention_days: 30,
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

        let admin_password_hash = match env("EARFUL_ADMIN_PASSWORD_HASH_FILE") {
            Some(path) => Some(
                std::fs::read_to_string(&path)
                    .with_context(|| format!("reading EARFUL_ADMIN_PASSWORD_HASH_FILE {path}"))?
                    .trim()
                    .to_string(),
            ),
            None => env("EARFUL_ADMIN_PASSWORD_HASH"),
        }
        .filter(|s| !s.is_empty());

        let embed = embed_config_from(&var)?;

        Ok(Self {
            bind: parse_or("EARFUL_BIND", "127.0.0.1:8080".parse()?)?,
            database_path: env("EARFUL_DATABASE_PATH")
                .unwrap_or_else(|| "earful.db".into())
                .into(),
            admin_password_hash,
            session_ttl_hours: parse_or("EARFUL_SESSION_TTL_HOURS", 24 * 7)?,
            cookie_secure: parse_or("EARFUL_COOKIE_SECURE", true)?,
            trusted_proxy_hops: parse_or("EARFUL_TRUSTED_PROXY_HOPS", 0)?,
            limits: Limits {
                per_ip_project_minute: parse_or(
                    "EARFUL_LIMIT_IP_PROJECT_MINUTE",
                    limits_default.per_ip_project_minute,
                )?,
                per_ip_project_hour: parse_or(
                    "EARFUL_LIMIT_IP_PROJECT_HOUR",
                    limits_default.per_ip_project_hour,
                )?,
                per_ip_project_day: parse_or(
                    "EARFUL_LIMIT_IP_PROJECT_DAY",
                    limits_default.per_ip_project_day,
                )?,
                per_ip_global_day: parse_or(
                    "EARFUL_LIMIT_IP_GLOBAL_DAY",
                    limits_default.per_ip_global_day,
                )?,
                per_project_hour: parse_or(
                    "EARFUL_LIMIT_PROJECT_HOUR",
                    limits_default.per_project_hour,
                )?,
                global_day: parse_or("EARFUL_LIMIT_GLOBAL_DAY", limits_default.global_day)?,
                spam_retention_days: parse_or(
                    "EARFUL_SPAM_RETENTION_DAYS",
                    limits_default.spam_retention_days,
                )?,
                challenges_per_ip_hour: parse_or(
                    "EARFUL_LIMIT_CHALLENGES_IP_HOUR",
                    limits_default.challenges_per_ip_hour,
                )?,
                logins_per_ip_window: limits_default.logins_per_ip_window,
                login_window_secs: limits_default.login_window_secs,
                min_fill_secs: parse_or("EARFUL_MIN_FILL_SECS", limits_default.min_fill_secs)?,
                spam_threshold: parse_or("EARFUL_SPAM_THRESHOLD", limits_default.spam_threshold)?,
            },
            pow: PowConfig {
                base_bits: parse_or("EARFUL_POW_BITS", pow_default.base_bits)?.min(28),
                // The widget refuses challenges above 28 bits (hostile-server guard).
                max_bits: parse_or("EARFUL_POW_MAX_BITS", pow_default.max_bits)?.min(28),
                challenge_ttl_secs: parse_or(
                    "EARFUL_CHALLENGE_TTL_SECS",
                    pow_default.challenge_ttl_secs,
                )?,
            },
            embed,
        })
    }
}

fn env(key: &str) -> Option<String> {
    var(key).map(|v| v.trim().to_string())
}

/// Reads an `EARFUL_*` variable. Deployments from before the rename may still
/// set the old `FH_*` name; an explicitly set legacy name wins, so an image
/// default for the new name can never silently override an existing setting.
pub fn var(key: &str) -> Option<String> {
    if let Some(rest) = key.strip_prefix("EARFUL_")
        && let Ok(v) = std::env::var(format!("FH_{rest}"))
    {
        return Some(v);
    }
    std::env::var(key).ok()
}

/// Names of legacy `FH_*` variables still set in the environment.
pub fn legacy_vars_in_use() -> Vec<String> {
    let mut keys: Vec<String> = std::env::vars()
        .map(|(k, _)| k)
        .filter(|k| k.starts_with("FH_"))
        .collect();
    keys.sort();
    keys
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn resolve(pairs: &[(&str, &str)]) -> Result<EmbedConfig> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        embed_config_from(&|k| map.get(k).cloned())
    }

    fn fake_model_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for f in MODEL2VEC_FILES {
            std::fs::write(dir.path().join(f), b"x").unwrap();
        }
        dir
    }

    #[test]
    fn auto_falls_back_to_hash_without_a_model() {
        let cfg = resolve(&[("EARFUL_EMBED_MODEL_PATH", "/definitely/not/here")]).unwrap();
        assert!(matches!(cfg, EmbedConfig::Hash { dim: 384 }));
        let partial = tempfile::tempdir().unwrap();
        std::fs::write(partial.path().join("config.json"), b"{}").unwrap();
        let cfg =
            resolve(&[("EARFUL_EMBED_MODEL_PATH", partial.path().to_str().unwrap())]).unwrap();
        assert!(
            matches!(cfg, EmbedConfig::Hash { .. }),
            "incomplete model dirs are ignored"
        );
    }

    #[test]
    fn auto_picks_model2vec_when_the_model_exists() {
        let dir = fake_model_dir();
        let cfg = resolve(&[("EARFUL_EMBED_MODEL_PATH", dir.path().to_str().unwrap())]).unwrap();
        match cfg {
            EmbedConfig::Model2Vec {
                path,
                repo,
                auto_download,
            } => {
                assert_eq!(path, dir.path());
                assert_eq!(repo, DEFAULT_MODEL2VEC_REPO);
                assert!(!auto_download);
            }
            other => panic!("expected model2vec, got {other:?}"),
        }
    }

    #[test]
    fn explicit_providers_parse() {
        assert!(matches!(
            resolve(&[("EARFUL_EMBED_PROVIDER", "none")]).unwrap(),
            EmbedConfig::Disabled
        ));
        assert!(matches!(
            resolve(&[
                ("EARFUL_EMBED_PROVIDER", "hash"),
                ("EARFUL_EMBED_DIM", "128")
            ])
            .unwrap(),
            EmbedConfig::Hash { dim: 128 }
        ));
        // Explicit model2vec keeps the config even when the model is missing,
        // so startup can fail loudly instead of silently degrading.
        match resolve(&[
            ("EARFUL_EMBED_PROVIDER", "model2vec"),
            ("EARFUL_EMBED_MODEL_PATH", "/nope"),
            ("EARFUL_EMBED_MODEL", "minishlab/potion-retrieval-32M"),
            ("EARFUL_EMBED_AUTO_DOWNLOAD", "true"),
        ])
        .unwrap()
        {
            EmbedConfig::Model2Vec {
                path,
                repo,
                auto_download,
            } => {
                assert_eq!(path, PathBuf::from("/nope"));
                assert_eq!(repo, "minishlab/potion-retrieval-32M");
                assert!(auto_download);
            }
            other => panic!("expected model2vec, got {other:?}"),
        }
        match resolve(&[
            ("EARFUL_EMBED_PROVIDER", "openai"),
            ("EARFUL_EMBED_API_KEY", "k"),
        ])
        .unwrap()
        {
            EmbedConfig::OpenAi {
                url,
                model,
                api_key,
            } => {
                assert_eq!(url, "https://api.openai.com/v1");
                assert_eq!(model, "text-embedding-3-small");
                assert_eq!(api_key.as_deref(), Some("k"));
            }
            other => panic!("expected openai, got {other:?}"),
        }
    }

    #[test]
    fn rejects_bad_values() {
        assert!(resolve(&[("EARFUL_EMBED_PROVIDER", "magic")]).is_err());
        assert!(resolve(&[("EARFUL_EMBED_DIM", "big")]).is_err());
        assert!(
            resolve(&[
                ("EARFUL_EMBED_PROVIDER", "model2vec"),
                ("EARFUL_EMBED_AUTO_DOWNLOAD", "yes")
            ])
            .is_err()
        );
    }
}
