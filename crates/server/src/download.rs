//! Fetches Model2Vec model files from the Hugging Face Hub.

use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::config::MODEL2VEC_FILES;

pub struct Downloaded {
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: String,
}

/// Smallest plausible size per file, to catch HTML error pages and truncated downloads.
fn min_size(file: &str) -> u64 {
    match file {
        "model.safetensors" => 1_000_000,
        "tokenizer.json" => 1_000,
        _ => 10,
    }
}

/// The default model, pinned to the same revision and hashes as the Dockerfile.
pub const PINNED_REPO: &str = "minishlab/potion-base-8M";
pub const PINNED_REVISION: &str = "bf8b056651a2c21b8d2565580b8569da283cab23";
const PINNED_SHA256: &[(&str, &str)] = &[
    (
        "config.json",
        "2a6ac0e9aaa356a68a5688070db78fc3a464fefe85d2f06a1905ce3718687553",
    ),
    (
        "model.safetensors",
        "f65d0f325faadc1e121c319e2faa41170d3fa07d8c89abd48ca5358d9a223de2",
    ),
    (
        "tokenizer.json",
        "e67e803f624fb4d67dea1c730d06e1067e1b14d830e2c2202569e3ef0f70bb50",
    ),
];

/// Resolves the revision to fetch and the hashes to enforce. The default model
/// at "main" (or its pinned commit) is fetched at the pinned commit and checked
/// file by file; anything else is unpinned and only size checked.
fn resolve(
    repo: &str,
    revision: &str,
) -> (String, Option<&'static [(&'static str, &'static str)]>) {
    if repo == PINNED_REPO && (revision == "main" || revision == PINNED_REVISION) {
        (PINNED_REVISION.to_string(), Some(PINNED_SHA256))
    } else {
        (revision.to_string(), None)
    }
}

pub fn file_url(repo: &str, revision: &str, file: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/{revision}/{file}")
}

/// Downloads `config.json`, `model.safetensors` and `tokenizer.json` into `dir`.
/// Each file is written to a temp file and renamed into place, so an
/// interrupted download never leaves a half written model behind.
pub async fn download_model(repo: &str, revision: &str, dir: &Path) -> Result<Vec<Downloaded>> {
    if repo.split('/').count() != 2 || repo.contains("..") {
        bail!("expected a Hugging Face repo id like minishlab/potion-base-8M, got {repo:?}");
    }
    let (revision, pinned) = resolve(repo, revision);
    let revision = revision.as_str();
    if pinned.is_none() {
        tracing::warn!(%repo, %revision, "downloading an unpinned model: files are not integrity checked");
    }
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()?;
    let mut out = Vec::new();
    for file in MODEL2VEC_FILES {
        let url = file_url(repo, revision, file);
        let mut resp = client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("fetching {url}"))?;
        if !resp.status().is_success() {
            bail!("fetching {url} returned {}", resp.status());
        }
        let tmp: PathBuf = dir.join(format!(".{file}.part"));
        let mut f =
            std::fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        let mut hasher = Sha256::new();
        let mut bytes = 0u64;
        while let Some(chunk) = resp
            .chunk()
            .await
            .with_context(|| format!("reading {url}"))?
        {
            hasher.update(&chunk);
            f.write_all(&chunk)?;
            bytes += chunk.len() as u64;
        }
        f.sync_all()?;
        drop(f);
        if bytes < min_size(file) {
            let _ = std::fs::remove_file(&tmp);
            bail!("{file} is only {bytes} bytes, the download looks broken");
        }
        let sha256 = hex::encode(hasher.finalize());
        if let Some(expected) = pinned.and_then(|p| p.iter().find(|(f, _)| f == file))
            && expected.1 != sha256
        {
            let _ = std::fs::remove_file(&tmp);
            bail!(
                "{file} sha256 {sha256} does not match the pinned {}",
                expected.1
            );
        }
        std::fs::rename(&tmp, dir.join(file))?;
        out.push(Downloaded {
            file,
            bytes,
            sha256,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_hub_urls() {
        assert_eq!(
            file_url("minishlab/potion-base-8M", "main", "config.json"),
            "https://huggingface.co/minishlab/potion-base-8M/resolve/main/config.json"
        );
    }

    #[test]
    fn default_model_is_pinned() {
        let (rev, pins) = resolve(PINNED_REPO, "main");
        assert_eq!(rev, PINNED_REVISION);
        assert_eq!(pins.unwrap().len(), MODEL2VEC_FILES.len());
        let (rev, pins) = resolve("minishlab/potion-retrieval-32M", "main");
        assert_eq!(rev, "main");
        assert!(pins.is_none());
    }

    #[tokio::test]
    async fn rejects_bad_repo_ids() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            download_model("no-slash", "main", dir.path())
                .await
                .is_err()
        );
        assert!(
            download_model("../../etc", "main", dir.path())
                .await
                .is_err()
        );
    }
}
