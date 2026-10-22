mod common;

use axum::http::{Method, StatusCode};
use common::{App, key_of};
use serde_json::json;

#[tokio::test]
async fn healthz_and_root_redirect() {
    let app = App::new().await;
    let r = app.get("/healthz").send().await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json(), json!({ "ok": true }));
    let r = app.get("/").send().await;
    assert!(r.status.is_redirection());
    assert_eq!(r.header("location"), Some("/admin/"));
    let r = app.get("/admin").send().await;
    assert_eq!(r.header("location"), Some("/admin/"));
}

#[tokio::test]
async fn security_headers_everywhere() {
    let app = App::new().await;
    for path in [
        "/healthz",
        "/api/admin/me",
        "/admin/",
        "/api/v1/challenge?key=x",
    ] {
        let r = app.get(path).send().await;
        assert_eq!(
            r.header("x-content-type-options"),
            Some("nosniff"),
            "{path}"
        );
        assert_eq!(r.header("referrer-policy"), Some("no-referrer"), "{path}");
        assert_eq!(r.header("x-frame-options"), Some("DENY"), "{path}");
        let csp = r.header("content-security-policy").unwrap();
        assert!(
            csp.contains("default-src 'self'") && csp.contains("frame-ancestors 'none'"),
            "{path}: {csp}"
        );
    }
}

#[tokio::test]
async fn hosted_page_escapes_and_is_frameable() {
    let app = App::admin().await;
    let p = app
        .post("/api/admin/projects")
        .json(json!({ "title": "<script>alert(1)</script>", "slug": "xss", "description": "\"quoted\" & <b>bold</b>" }))
        .send()
        .await
        .json();
    let r = app.get("/s/xss").anonymous().send().await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.header("content-type").unwrap().starts_with("text/html"));
    assert!(!r.text.contains("<script>alert(1)</script>"));
    assert!(r.text.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(
        r.text
            .contains("&quot;quoted&quot; &amp; &lt;b&gt;bold&lt;/b&gt;")
    );
    assert!(r.text.contains(&format!("data-fh-key=\"{}\"", key_of(&p))));
    assert!(r.text.contains("data-fh-inline"));
    assert!(
        r.text
            .contains("<script src=\"/widget.js\" defer></script>")
    );
    assert_eq!(r.header("x-frame-options"), None);
    let csp = r.header("content-security-policy").unwrap();
    assert!(!csp.contains("frame-ancestors"), "{csp}");
    assert!(csp.contains("script-src 'self'"), "{csp}");

    app.patch(&format!("/api/admin/projects/{}", p["id"]))
        .json(json!({ "accepting": false }))
        .send()
        .await;
    let paused = app.get("/s/xss").anonymous().send().await;
    assert_eq!(paused.status, StatusCode::OK);
    assert!(paused.text.contains("not accepting feedback"));
    assert!(!paused.text.contains("data-fh-key"));

    assert_eq!(
        app.get("/s/nope").anonymous().send().await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn cors_preflight_only_on_public_api() {
    let app = App::new().await;
    let r = app
        .req(Method::OPTIONS, "/api/v1/feedback")
        .anonymous()
        .header("origin", "https://game.example")
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "content-type")
        .send()
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.header("access-control-allow-origin"), Some("*"));
    assert!(
        r.header("access-control-allow-methods")
            .unwrap()
            .contains("POST")
    );
    assert!(r.header("access-control-allow-credentials").is_none());

    let r = app
        .get("/api/v1/challenge?key=x")
        .header("origin", "https://game.example")
        .send()
        .await;
    assert_eq!(r.header("access-control-allow-origin"), Some("*"));

    let r = app
        .req(Method::OPTIONS, "/api/admin/projects")
        .header("origin", "https://evil.example")
        .header("access-control-request-method", "POST")
        .send()
        .await;
    assert!(r.header("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn admin_spa_and_widget_degrade_or_serve() {
    let app = App::new().await;
    // Works whether or not `pnpm build:web` has run.
    let index = app.get("/admin/").send().await;
    assert!(index.status == StatusCode::OK || index.status == StatusCode::SERVICE_UNAVAILABLE);
    if index.status == StatusCode::OK {
        assert_eq!(index.header("cache-control"), Some("no-cache"));
        let deep = app.get("/admin/projects/1").send().await;
        assert_eq!(
            deep.status,
            StatusCode::OK,
            "client side routes fall back to index.html"
        );
    } else {
        assert!(index.text.contains("pnpm build:web"));
    }
    assert_eq!(
        app.get("/admin/missing.js").send().await.status,
        StatusCode::NOT_FOUND
    );
    let w = app.get("/widget.js").send().await;
    assert!(w.header("content-type").unwrap().contains("javascript"));
    if w.status == StatusCode::OK {
        assert_eq!(w.header("access-control-allow-origin"), Some("*"));
    }
}

#[tokio::test]
async fn admin_assets_do_not_allow_path_traversal() {
    let app = common::App::new().await;
    for path in [
        "/admin/..%2f..%2f..%2fCargo.toml",
        "/admin/%2e%2e/%2e%2e/%2e%2e/Cargo.toml",
        "/admin/..%5c..%5c..%5cCargo.toml",
        "/admin/assets/..%2f..%2f..%2f..%2fCargo.toml",
    ] {
        let r = app.get(path).anonymous().send().await;
        assert!(!r.text.contains("[workspace]"), "{path} leaked a file");
    }
}
