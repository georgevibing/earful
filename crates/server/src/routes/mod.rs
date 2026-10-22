//! HTTP surface: public ingestion, admin API, hosted page and static assets.

pub(crate) mod admin;
mod assets;
pub(crate) mod public;

use std::net::SocketAddr;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{ConnectInfo, DefaultBodyLimit, FromRequest, FromRequestParts, Path, Request},
    http::{HeaderName, HeaderValue, Method, header, request::Parts},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::de::DeserializeOwned;
use serde_json::json;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::{error::AppError, spam, state::AppState};

const CSP_BASE: &str = "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; \
     script-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'";

pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, HeaderName::from_static("x-fh-key")]);

    let v1 = Router::new()
        .route("/challenge", get(public::challenge))
        .route("/feedback", post(public::submit))
        .layer(DefaultBodyLimit::max(32 * 1024))
        .layer(cors);

    Router::new()
        .nest("/api/v1", v1)
        .nest(
            "/api/admin",
            admin::router().layer(DefaultBodyLimit::max(1 << 20)),
        )
        .route(
            "/mcp",
            post(crate::mcp::handle).layer(DefaultBodyLimit::max(1 << 20)),
        )
        .route("/s/{slug}", get(public::hosted))
        .route("/widget.js", get(assets::widget))
        .route("/favicon.ico", get(assets::root_icon))
        .route("/favicon.svg", get(assets::root_icon))
        .route("/apple-touch-icon.png", get(assets::root_icon))
        .route("/admin", get(|| async { Redirect::permanent("/admin/") }))
        .route("/admin/", get(assets::admin_index))
        .route("/admin/{*path}", get(assets::admin))
        .route("/healthz", get(|| async { Json(json!({ "ok": true })) }))
        .route("/", get(|| async { Redirect::temporary("/admin/") }))
        .layer(middleware::from_fn(security_headers))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn security_headers(req: Request, next: Next) -> Response {
    let hosted = req.uri().path().starts_with("/s/");
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if hosted {
        h.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP_BASE),
        );
    } else {
        h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
        h.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_str(&format!("{CSP_BASE}; frame-ancestors 'none'"))
                .expect("static csp"),
        );
    }
    resp
}

/// Pseudonymised client IP (see [`spam::client_ip`] for proxy handling).
pub struct ClientIp(pub String);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0);
        let ip = spam::client_ip(&parts.headers, peer, state.cfg.trusted_proxy_hops);
        Ok(Self(state.ip_hash(&ip)))
    }
}

/// JSON body extractor whose failures use our error format. Content type is not
/// enforced so `navigator.sendBeacon` and `text/plain` posts work too.
pub struct JsonBody<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for JsonBody<T> {
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(IntoResponse::into_response)?;
        serde_json::from_slice(&bytes).map(JsonBody).map_err(|e| {
            AppError::bad("invalid_json", format!("invalid JSON body: {e}")).into_response()
        })
    }
}

/// Numeric path id; anything else is a plain 404.
pub struct IdPath(pub i64);

impl<S: Send + Sync> FromRequestParts<S> for IdPath {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Path::<i64>::from_request_parts(parts, state)
            .await
            .map(|p| IdPath(p.0))
            .map_err(|_| AppError::NotFound)
    }
}

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn escapes_html() {
        assert_eq!(
            super::html_escape("<a href=\"x\">'&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
    }
}
