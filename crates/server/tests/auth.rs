mod common;

use axum::http::{Method, StatusCode};
use common::{App, PASSWORD};
use serde_json::json;

#[tokio::test]
async fn admin_routes_require_a_session() {
    let app = App::new().await;
    let routes = [
        (Method::GET, "/api/admin/me"),
        (Method::GET, "/api/admin/projects"),
        (Method::POST, "/api/admin/projects"),
        (Method::GET, "/api/admin/projects/1"),
        (Method::PATCH, "/api/admin/projects/1"),
        (Method::DELETE, "/api/admin/projects/1"),
        (Method::POST, "/api/admin/projects/1/rotate-key"),
        (Method::GET, "/api/admin/projects/1/export"),
        (Method::GET, "/api/admin/feedback"),
        (Method::POST, "/api/admin/feedback/bulk"),
        (Method::GET, "/api/admin/feedback/1"),
        (Method::PATCH, "/api/admin/feedback/1"),
        (Method::DELETE, "/api/admin/feedback/1"),
        (Method::GET, "/api/admin/stats"),
        (Method::POST, "/api/admin/reindex"),
    ];
    for (method, path) in routes {
        let r = app.req(method.clone(), path).json(json!({})).send().await;
        assert_eq!(
            r.status,
            StatusCode::UNAUTHORIZED,
            "{method} {path}: {}",
            r.text
        );
        assert_eq!(r.json()["error"], "unauthorized");
    }
    // A forged cookie is not a session.
    let r = app
        .get("/api/admin/me")
        .header("cookie", &format!("earful_session={}", "a".repeat(64)))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_sets_a_hardened_cookie_and_logout_revokes_it() {
    let mut app = App::new().await;
    let wrong = app
        .post("/api/admin/login")
        .json(json!({ "password": "nope" }))
        .send()
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.json()["error"], "invalid_credentials");

    let r = app
        .post("/api/admin/login")
        .json(json!({ "password": PASSWORD }))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let cookie = r.header("set-cookie").unwrap().to_string();
    assert!(cookie.starts_with("earful_session="), "{cookie}");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.contains("SameSite=Strict"), "{cookie}");
    assert!(cookie.contains("Path=/"), "{cookie}");
    app.cookie = Some(cookie.split(';').next().unwrap().to_string());

    assert_eq!(app.get("/api/admin/me").send().await.status, StatusCode::OK);
    let out = app.post("/api/admin/logout").send().await;
    assert_eq!(out.status, StatusCode::NO_CONTENT);
    assert!(out.header("set-cookie").unwrap().contains("Max-Age=0"));
    assert_eq!(
        app.get("/api/admin/me").send().await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn secure_flag_follows_config() {
    let app = App::with(|c| c.cookie_secure = true).await;
    let r = app
        .post("/api/admin/login")
        .json(json!({ "password": PASSWORD }))
        .send()
        .await;
    assert!(r.header("set-cookie").unwrap().contains("Secure"));
}

#[tokio::test]
async fn failed_logins_are_rate_limited_per_ip() {
    let app = App::new().await;
    for _ in 0..5 {
        let r = app
            .post("/api/admin/login")
            .ip("5.5.5.5")
            .json(json!({ "password": "x" }))
            .send()
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    let r = app
        .post("/api/admin/login")
        .ip("5.5.5.5")
        .json(json!({ "password": PASSWORD }))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(r.header("retry-after").unwrap().parse::<i64>().unwrap() > 0);
    // Another IP is unaffected.
    let r = app
        .post("/api/admin/login")
        .ip("6.6.6.6")
        .json(json!({ "password": PASSWORD }))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn successful_logins_do_not_consume_the_budget() {
    let app = App::new().await;
    for _ in 0..8 {
        let r = app
            .post("/api/admin/login")
            .json(json!({ "password": PASSWORD }))
            .send()
            .await;
        assert_eq!(r.status, StatusCode::NO_CONTENT);
    }
}

#[tokio::test]
async fn cross_origin_writes_are_rejected() {
    let app = App::admin().await;
    let r = app
        .post("/api/admin/projects")
        .header("origin", "https://evil.example")
        .json(json!({ "title": "X" }))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.json()["error"], "bad_origin");
    let r = app
        .post("/api/admin/projects")
        .header("origin", "https://fh.test")
        .json(json!({ "title": "X" }))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let r = app
        .post("/api/admin/login")
        .header("origin", "https://evil.example")
        .json(json!({ "password": PASSWORD }))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn missing_admin_hash_fails_closed() {
    let app = App::with(|c| c.admin_password_hash = None).await;
    for pw in ["", PASSWORD, "admin"] {
        let r = app
            .post("/api/admin/login")
            .ip("7.7.7.7")
            .json(json!({ "password": pw }))
            .send()
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{pw}");
    }
}

#[tokio::test]
async fn malformed_login_body_is_a_json_error() {
    let app = App::new().await;
    let r = app
        .post("/api/admin/login")
        .json(json!({ "nope": 1 }))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["error"], "invalid_json");
}

#[tokio::test]
async fn concurrent_wrong_passwords_cannot_bypass_the_login_limit() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    let app = App::new().await;
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..20 {
        let router = app.router.clone();
        set.spawn(async move {
            let req = Request::builder()
                .method(Method::POST)
                .uri("/api/admin/login")
                .header("host", "fh.test")
                .header("x-forwarded-for", "7.7.7.7")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"password":"guess"}"#))
                .unwrap();
            router.oneshot(req).await.unwrap().status()
        });
    }
    let mut guesses = 0;
    while let Some(status) = set.join_next().await {
        let status = status.unwrap();
        if status == StatusCode::UNAUTHORIZED {
            guesses += 1;
        } else {
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        }
    }
    assert!(guesses <= 5, "{guesses} password guesses got through");
}
