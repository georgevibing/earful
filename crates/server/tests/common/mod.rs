//! Test harness: an in-process router over a temp database.
#![allow(dead_code)]

use std::sync::OnceLock;

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use earful::{auth, build_router, config::Config, pow, state::AppState};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

pub const PASSWORD: &str = "correct horse battery staple";

fn password_hash() -> String {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| auth::hash_password(PASSWORD).unwrap())
        .clone()
}

pub struct App {
    pub router: Router,
    pub state: AppState,
    pub cookie: Option<String>,
    _dir: TempDir,
}

pub struct Resp {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub text: String,
}

impl Resp {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or_else(|e| panic!("not json ({e}): {}", self.text))
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

/// Request builder with sensible defaults (JSON, client IP 1.1.1.1, admin cookie when logged in).
pub struct Req<'a> {
    app: &'a App,
    method: Method,
    path: String,
    body: Option<Value>,
    ip: String,
    headers: Vec<(String, String)>,
    with_cookie: bool,
    raw: Option<String>,
}

impl<'a> Req<'a> {
    pub fn json(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    pub fn ip(mut self, ip: &str) -> Self {
        self.ip = ip.to_string();
        self
    }

    pub fn header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.to_string(), v.to_string()));
        self
    }

    pub fn anonymous(mut self) -> Self {
        self.with_cookie = false;
        self
    }

    /// Sends `raw` as the body verbatim (for malformed JSON tests).
    pub async fn send_raw(mut self, raw: &str) -> Resp {
        self.body = None;
        self.raw = Some(raw.to_string());
        self.send().await
    }

    pub async fn send(self) -> Resp {
        let mut b = Request::builder()
            .method(self.method)
            .uri(&self.path)
            .header(header::HOST, "fh.test")
            .header("x-forwarded-for", &self.ip);
        if self.with_cookie
            && let Some(c) = &self.app.cookie
        {
            b = b.header(header::COOKIE, c);
        }
        for (k, v) in &self.headers {
            b = b.header(k, v);
        }
        let body = match (self.body, self.raw) {
            (Some(v), _) => {
                b = b.header(header::CONTENT_TYPE, "application/json");
                Body::from(v.to_string())
            }
            (None, Some(raw)) => Body::from(raw),
            (None, None) => Body::empty(),
        };
        let resp = self
            .app
            .router
            .clone()
            .oneshot(b.body(body).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        Resp {
            status,
            headers,
            text: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }
}

impl App {
    pub async fn new() -> Self {
        Self::with(|_| {}).await
    }

    pub async fn with(tweak: impl FnOnce(&mut Config)) -> Self {
        let dir = TempDir::new().unwrap();
        let mut cfg = Config::for_tests(dir.path().join("test.db"));
        cfg.admin_password_hash = Some(password_hash());
        tweak(&mut cfg);
        let state = AppState::init(cfg).await.unwrap();
        Self {
            router: build_router(state.clone()),
            state,
            cookie: None,
            _dir: dir,
        }
    }

    /// Logged in app, the common case for admin tests.
    pub async fn admin() -> Self {
        let mut app = Self::new().await;
        app.login().await;
        app
    }

    pub fn req(&self, method: Method, path: &str) -> Req<'_> {
        Req {
            app: self,
            method,
            path: path.to_string(),
            body: None,
            ip: "1.1.1.1".into(),
            headers: vec![],
            with_cookie: true,
            raw: None,
        }
    }

    pub fn get(&self, path: &str) -> Req<'_> {
        self.req(Method::GET, path)
    }

    pub fn post(&self, path: &str) -> Req<'_> {
        self.req(Method::POST, path)
    }

    pub fn patch(&self, path: &str) -> Req<'_> {
        self.req(Method::PATCH, path)
    }

    pub fn delete(&self, path: &str) -> Req<'_> {
        self.req(Method::DELETE, path)
    }

    pub async fn login(&mut self) {
        let r = self
            .post("/api/admin/login")
            .ip("9.9.9.9")
            .json(json!({ "password": PASSWORD }))
            .send()
            .await;
        assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.text);
        let set_cookie = r.header("set-cookie").expect("session cookie").to_string();
        self.cookie = Some(set_cookie.split(';').next().unwrap().to_string());
    }

    pub async fn create_project(&self, title: &str) -> Value {
        let r = self
            .post("/api/admin/projects")
            .json(json!({ "title": title }))
            .send()
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
        r.json()
    }

    pub async fn challenge(&self, key: &str, ip: &str) -> Resp {
        self.get(&format!("/api/v1/challenge?key={key}"))
            .anonymous()
            .ip(ip)
            .send()
            .await
    }

    /// Fetches a challenge, solves it and posts `body` merged with key, challenge and nonce.
    pub async fn submit(&self, key: &str, ip: &str, mut body: Value) -> Resp {
        let ch = self.challenge(key, ip).await;
        assert_eq!(ch.status, StatusCode::OK, "{}", ch.text);
        let ch = ch.json();
        let token = ch["challenge"].as_str().unwrap().to_string();
        let nonce = pow::solve(&token, ch["bits"].as_u64().unwrap() as u32);
        let obj = body.as_object_mut().unwrap();
        obj.insert("key".into(), json!(key));
        obj.insert("challenge".into(), json!(token));
        obj.insert("nonce".into(), json!(nonce));
        self.post("/api/v1/feedback")
            .anonymous()
            .ip(ip)
            .json(body)
            .send()
            .await
    }

    /// Submits a valid bug report and asserts it was accepted.
    pub async fn submit_ok(&self, key: &str, ip: &str, kind: &str, title: &str, body: &str) {
        let r = self
            .submit(
                key,
                ip,
                json!({ "kind": kind, "title": title, "body": body }),
            )
            .await;
        assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.text);
    }

    pub async fn feedback(&self, query: &str) -> Value {
        let r = self
            .get(&format!("/api/admin/feedback?{query}"))
            .send()
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
        r.json()
    }

    pub async fn embed_all(&self) {
        while earful::worker::embed_pending(&self.state).await.unwrap() > 0 {}
    }

    pub async fn sql_i64(&self, sql: &'static str) -> i64 {
        self.state
            .db
            .call(move |c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .await
            .unwrap()
    }
}

pub fn key_of(project: &Value) -> String {
    project["ingest_key"].as_str().unwrap().to_string()
}

pub fn ids(list: &Value) -> Vec<i64> {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_i64().unwrap())
        .collect()
}

pub fn titles(list: &Value) -> Vec<String> {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["title"].as_str().unwrap().to_string())
        .collect()
}
