//! `feedback-harvest mcp-stdio`: bridges a stdio MCP client to a remote `/mcp`.
//!
//! Reads newline delimited JSON-RPC from stdin, forwards each message over
//! HTTPS with the bearer token and writes the reply to stdout. Logs go to stderr.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub async fn run() -> Result<()> {
    let url = std::env::var("FH_MCP_URL")
        .context("set FH_MCP_URL, e.g. https://feedback.example.com/mcp")?;
    let token = match std::env::var("FH_MCP_TOKEN_FILE") {
        Ok(path) => std::fs::read_to_string(&path)
            .with_context(|| format!("reading FH_MCP_TOKEN_FILE {path}"))?
            .trim()
            .to_string(),
        Err(_) => std::env::var("FH_MCP_TOKEN")
            .context("set FH_MCP_TOKEN (or FH_MCP_TOKEN_FILE) to an fht_ token")?,
    };
    if !url.starts_with("https://") && !is_loopback(&url) {
        bail!("FH_MCP_URL must use https:// (plain http is only allowed for localhost)");
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    let mut session: Option<String> = None;
    let mut version: Option<String> = None;

    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let id = serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|v| v.get("id").cloned());
        let mut req = client
            .post(&url)
            .bearer_auth(&token)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(line.to_string());
        if let Some(s) = &session {
            req = req.header("mcp-session-id", s);
        }
        if let Some(v) = &version {
            req = req.header("mcp-protocol-version", v);
        }

        let reply = match req.send().await {
            Ok(resp) => {
                if let Some(s) = resp
                    .headers()
                    .get("mcp-session-id")
                    .and_then(|v| v.to_str().ok())
                {
                    session = Some(s.to_string());
                }
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                if status.as_u16() == 202 || body.trim().is_empty() {
                    None
                } else if let Ok(v) = serde_json::from_str::<Value>(&body)
                    && (v.get("jsonrpc").is_some() || v.is_array())
                {
                    if let Some(pv) = v.pointer("/result/protocolVersion").and_then(Value::as_str) {
                        version = Some(pv.to_string());
                    }
                    Some(v)
                } else {
                    id.clone()
                        .map(|id| error(id, &format!("HTTP {status}: {}", body.trim())))
                }
            }
            Err(e) => id
                .clone()
                .map(|id| error(id, &format!("request failed: {e}"))),
        };
        if let Some(v) = reply {
            stdout
                .write_all(serde_json::to_string(&v)?.as_bytes())
                .await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

fn error(id: Value, message: &str) -> Value {
    eprintln!("feedback-harvest mcp-stdio: {message}");
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": message } })
}

fn is_loopback(url: &str) -> bool {
    let rest = url.strip_prefix("http://").unwrap_or_default();
    ["127.0.0.1", "localhost", "[::1]"]
        .iter()
        .any(|h| rest.starts_with(h) && rest[h.len()..].starts_with([':', '/']))
}

#[cfg(test)]
mod tests {
    #[test]
    fn loopback_detection() {
        assert!(super::is_loopback("http://127.0.0.1:8080/mcp"));
        assert!(super::is_loopback("http://localhost/mcp"));
        assert!(!super::is_loopback("http://localhost.evil.com/mcp"));
        assert!(!super::is_loopback("http://example.com/mcp"));
    }
}
