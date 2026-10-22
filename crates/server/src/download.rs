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
        std::fs::rename(&tmp, dir.join(file))?;
        out.push(Downloaded {
            file,
            bytes,
            sha256: hex::encode(hasher.finalize()),
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
