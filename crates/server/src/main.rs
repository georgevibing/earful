use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    process::ExitCode,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use feedback_harvest::{
    auth, build_router,
    config::{Config, DEFAULT_MODEL2VEC_PATH, DEFAULT_MODEL2VEC_REPO},
    download, embed, pow,
    state::AppState,
    worker,
};
use tracing_subscriber::EnvFilter;

const HELP: &str = "feedback-harvest: tiny self-hosted feedback and bug report collector

USAGE:
    feedback-harvest [COMMAND]

COMMANDS:
    serve                      Run the HTTP server (default)
    hash-password              Read a password from stdin, print its argon2id hash
                               for FH_ADMIN_PASSWORD_HASH
    healthcheck                Exit 0 if the local server answers /healthz (for Docker)
    solve <challenge> <bits>   Print a proof of work nonce for a challenge (for testing)
    download-model [repo] [dir] [--revision <rev>]
                               Fetch a Model2Vec model for offline semantic search
                               (default minishlab/potion-base-8M into
                               models/potion-base-8M, revision main)
    mcp-stdio                  Bridge a stdio MCP client to a remote /mcp endpoint
                               (env FH_MCP_URL and FH_MCP_TOKEN or FH_MCP_TOKEN_FILE)
    help, --help, -h           Show this help

Configuration is read from FH_* environment variables, see README.md.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("serve") => serve(),
        Some("hash-password") => hash_password(),
        Some("healthcheck") => healthcheck(),
        Some("solve") => solve(&args[1..]),
        Some("download-model") => download_model(&args[1..]),
        Some("mcp-stdio") => mcp_stdio(),
        Some("help" | "--help" | "-h") => {
            println!("{HELP}");
            Ok(())
        }
        Some(other) => {
            eprintln!("unknown command {other:?}\n\n{HELP}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn serve() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let cfg = Config::from_env()?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(cfg))
}

async fn run(cfg: Config) -> Result<()> {
    if cfg.admin_password_hash.is_none() {
        tracing::warn!(
            "FH_ADMIN_PASSWORD_HASH is not set: every admin login will be rejected. \
             Generate one with `feedback-harvest hash-password`."
        );
    }
    let bind = cfg.bind;
    let state = AppState::init(cfg).await?;
    worker::spawn(state.clone());
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    tracing::info!(addr = %listener.local_addr()?, "listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    tracing::info!("shut down");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = term => {}
    }
}

fn mcp_stdio() -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(feedback_harvest::mcp::bridge::run())
}

fn download_model(args: &[String]) -> Result<()> {
    let mut positional = Vec::new();
    let mut revision = "main".to_string();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--revision" => {
                revision = it.next().context("--revision needs a value")?.clone();
            }
            other if other.starts_with("--") => bail!("unknown flag {other:?}"),
            other => positional.push(other.to_string()),
        }
    }
    if positional.len() > 2 {
        bail!("usage: feedback-harvest download-model [repo] [dir] [--revision <rev>]");
    }
    let repo = positional
        .first()
        .cloned()
        .unwrap_or_else(|| DEFAULT_MODEL2VEC_REPO.into());
    let dir = std::path::PathBuf::from(positional.get(1).cloned().unwrap_or_else(|| {
        match repo.rsplit_once('/') {
            Some((_, name)) if repo != DEFAULT_MODEL2VEC_REPO => format!("models/{name}"),
            _ => DEFAULT_MODEL2VEC_PATH.into(),
        }
    }));
    let files = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(download::download_model(&repo, &revision, &dir))?;
    for f in &files {
        println!("{:<18} {:>10} bytes  sha256 {}", f.file, f.bytes, f.sha256);
    }
    // Prove the files form a loadable model before declaring success.
    let model = embed::load_model2vec(&dir)?;
    let dim = model.encode_single("hello").len();
    println!(
        "loaded {repo}@{revision} into {} ({dim} dimensions)",
        dir.display()
    );
    println!("use it with FH_EMBED_MODEL_PATH={}", dir.display());
    Ok(())
}

fn hash_password() -> Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let password = input.trim_end_matches(['\n', '\r']);
    if password.chars().count() < 10 {
        bail!(
            "password must be at least 10 characters (pipe it in, e.g. `printf '%s' 'pw' | feedback-harvest hash-password`)"
        );
    }
    println!("{}", auth::hash_password(password)?);
    Ok(())
}

fn healthcheck() -> Result<()> {
    let bind = std::env::var("FH_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let port = bind.rsplit(':').next().unwrap_or("8080");
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse()?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(b"GET /healthz HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
    let mut buf = [0u8; 64];
    let n = stream.read(&mut buf)?;
    let head = String::from_utf8_lossy(&buf[..n]);
    if head.starts_with("HTTP/1.1 200") || head.starts_with("HTTP/1.0 200") {
        Ok(())
    } else {
        bail!("unhealthy: {}", head.lines().next().unwrap_or_default())
    }
}

fn solve(args: &[String]) -> Result<()> {
    let [challenge, bits] = args else {
        bail!("usage: feedback-harvest solve <challenge> <bits>");
    };
    let bits: u32 = bits.parse().context("bits must be a number")?;
    if bits > 32 {
        bail!("bits must be at most 32");
    }
    println!("{}", pow::solve(challenge, bits));
    Ok(())
}
