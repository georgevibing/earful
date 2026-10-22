//! Static assets compiled into the binary: the admin SPA and the widget bundle.
//! Folders may be missing (fresh checkout, Rust-only CI); the handlers degrade gracefully.

use axum::{
    extract::Path,
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../apps/admin/dist/"]
#[allow_missing = true]
struct AdminAssets;

#[derive(RustEmbed)]
#[folder = "../../packages/widget/dist/"]
#[allow_missing = true]
struct WidgetAssets;

fn file_response(
    data: std::borrow::Cow<'static, [u8]>,
    path: &str,
    cache: &'static str,
) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut resp = data.into_owned().into_response();
    let h = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(mime.as_ref()) {
        h.insert(header::CONTENT_TYPE, v);
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    resp
}

fn not_built() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Html(
            "<!doctype html><meta charset=\"utf-8\"><title>Admin UI not built</title>\
             <h1>Admin UI not built</h1><p>Run <code>pnpm build:web</code> and rebuild the server.</p>",
        ),
    )
        .into_response()
}

pub async fn admin_index() -> Response {
    match AdminAssets::get("index.html") {
        Some(f) => file_response(f.data, "index.html", "no-cache"),
        None => not_built(),
    }
}

pub async fn admin(Path(path): Path<String>) -> Response {
    if let Some(f) = AdminAssets::get(&path) {
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return file_response(f.data, &path, cache);
    }
    // Paths that look like files are real 404s; everything else is a client side route.
    let last = path.rsplit('/').next().unwrap_or_default();
    if last.contains('.') {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    admin_index().await
}

/// Icons browsers request at the site root (also used by the hosted page).
/// Served from the admin build so there is a single source of truth.
pub async fn root_icon(uri: axum::http::Uri) -> Response {
    let name = uri.path().trim_start_matches('/');
    match AdminAssets::get(name) {
        Some(f) => file_response(f.data, name, "public, max-age=86400"),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

pub async fn widget() -> Response {
    match WidgetAssets::get("widget.js") {
        Some(f) => {
            let mut resp = file_response(f.data, "widget.js", "public, max-age=300");
            resp.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            );
            resp
        }
        None => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/javascript")],
            "// widget not built: run `pnpm build:web` and rebuild the server\n",
        )
            .into_response(),
    }
}
