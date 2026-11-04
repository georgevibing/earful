//! MCP endpoint: tokens, transport, JSON-RPC errors, every tool, resources and prompts.

mod common;

use axum::http::StatusCode;
use common::{App, Resp};
use serde_json::{Value, json};

async fn token(app: &App, scope: &str) -> String {
    let r = app
        .post("/api/admin/tokens")
        .json(json!({ "name": format!("{scope} token"), "scope": scope }))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
    r.json()["token"].as_str().unwrap().to_string()
}

async fn rpc_raw(app: &App, tok: &str, body: Value) -> Resp {
    app.post("/mcp")
        .anonymous()
        .header("authorization", &format!("Bearer {tok}"))
        .json(body)
        .send()
        .await
}

async fn rpc(app: &App, tok: &str, method: &str, params: Value) -> Value {
    let r = rpc_raw(
        app,
        tok,
        json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    r.json()
}

/// Calls a tool and returns the full `result` (content, structuredContent, isError).
async fn tool(app: &App, tok: &str, name: &str, args: Value) -> Value {
    let v = rpc(
        app,
        tok,
        "tools/call",
        json!({ "name": name, "arguments": args }),
    )
    .await;
    assert!(v.get("error").is_none(), "{name}: {v}");
    v["result"].clone()
}

/// Calls a tool that must succeed and returns its structured content.
async fn ok(app: &App, tok: &str, name: &str, args: Value) -> Value {
    let r = tool(app, tok, name, args).await;
    assert_eq!(
        r["isError"], false,
        "{name} failed: {}",
        r["content"][0]["text"]
    );
    assert!(
        r["content"][0]["text"]
            .as_str()
            .is_some_and(|t| !t.is_empty())
    );
    r["structuredContent"].clone()
}

/// Calls a tool that must fail and returns its error text.
async fn err(app: &App, tok: &str, name: &str, args: Value) -> String {
    let r = tool(app, tok, name, args).await;
    assert_eq!(r["isError"], true, "{name} should fail: {r}");
    r["content"][0]["text"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn token_crud_never_exposes_secrets() {
    let app = App::admin().await;
    let r = app
        .post("/api/admin/tokens")
        .json(json!({ "name": "laptop", "scope": "write" }))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let created = r.json();
    let raw = created["token"].as_str().unwrap();
    assert!(raw.starts_with("fht_") && raw.len() == 68);
    assert_eq!(created["prefix"], &raw[..8]);
    assert_eq!(created["scope"], "write");
    assert!(created["last_used_at"].is_null());

    let list = app.get("/api/admin/tokens").send().await;
    assert_eq!(list.status, StatusCode::OK);
    assert!(!list.text.contains(&raw[8..]), "raw token leaked in list");
    assert!(!list.text.contains("token_hash"));
    assert_eq!(list.json().as_array().unwrap().len(), 1);

    for (body, code) in [
        (json!({ "name": "", "scope": "read" }), "invalid_name"),
        (json!({ "name": "x", "scope": "admin" }), "invalid_scope"),
        (json!({ "name": "x" }), "invalid_scope"),
    ] {
        let r = app.post("/api/admin/tokens").json(body).send().await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
        assert_eq!(r.json()["error"], code);
    }

    // Using the token updates last_used_at.
    rpc(&app, raw, "ping", json!({})).await;
    let list = app.get("/api/admin/tokens").send().await.json();
    assert!(list[0]["last_used_at"].is_i64());

    let id = created["id"].as_i64().unwrap();
    let r = app.delete(&format!("/api/admin/tokens/{id}")).send().await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = app.delete(&format!("/api/admin/tokens/{id}")).send().await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // Token admin needs a session.
    for r in [
        app.get("/api/admin/tokens").anonymous().send().await,
        app.post("/api/admin/tokens")
            .anonymous()
            .json(json!({ "name": "x", "scope": "read" }))
            .send()
            .await,
        app.delete("/api/admin/tokens/1").anonymous().send().await,
    ] {
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn bearer_auth_rejects_missing_invalid_and_revoked() {
    let app = App::admin().await;
    let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });

    // Missing token, even with an admin session cookie.
    let r = app.post("/mcp").json(ping.clone()).send().await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert!(r.header("www-authenticate").unwrap().starts_with("Bearer"));

    let r = rpc_raw(&app, &format!("fht_{}", "0".repeat(64)), ping.clone()).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = rpc_raw(&app, "garbage", ping.clone()).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    let tok = token(&app, "read").await;
    assert_eq!(
        rpc_raw(&app, &tok, ping.clone()).await.status,
        StatusCode::OK
    );
    let id = app.get("/api/admin/tokens").send().await.json()[0]["id"]
        .as_i64()
        .unwrap();
    app.delete(&format!("/api/admin/tokens/{id}")).send().await;
    assert_eq!(
        rpc_raw(&app, &tok, ping).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn failed_bearer_attempts_are_rate_limited_per_ip() {
    let app = App::admin().await;
    let good = token(&app, "read").await;
    let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });
    let bad = format!("fht_{}", "a".repeat(64));
    for _ in 0..20 {
        let r = app
            .post("/mcp")
            .anonymous()
            .ip("6.6.6.6")
            .header("authorization", &format!("Bearer {bad}"))
            .json(ping.clone())
            .send()
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    // The attacker IP is now locked out, even with a valid token.
    let r = app
        .post("/mcp")
        .anonymous()
        .ip("6.6.6.6")
        .header("authorization", &format!("Bearer {good}"))
        .json(ping.clone())
        .send()
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(r.header("retry-after").is_some());
    // Everyone else is unaffected.
    assert_eq!(rpc_raw(&app, &good, ping).await.status, StatusCode::OK);
}

#[tokio::test]
async fn initialize_negotiates_version_and_returns_session() {
    let app = App::admin().await;
    let tok = token(&app, "read").await;
    for (asked, expected) in [
        ("2025-11-25", "2025-11-25"),
        ("2025-06-18", "2025-06-18"),
        ("2025-03-26", "2025-03-26"),
        ("2024-11-05", "2025-06-18"),
    ] {
        let r = rpc_raw(
            &app,
            &tok,
            json!({ "jsonrpc": "2.0", "id": "init", "method": "initialize", "params": {
                "protocolVersion": asked, "capabilities": {}, "clientInfo": { "name": "test", "version": "1" }
            }}),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK);
        assert_eq!(r.header("mcp-session-id").map(str::len), Some(32));
        let v = r.json();
        assert_eq!(v["id"], "init");
        let res = &v["result"];
        assert_eq!(res["protocolVersion"], expected);
        assert_eq!(res["serverInfo"]["name"], "feedback-harvest");
        assert!(res["capabilities"]["tools"].is_object());
        assert!(res["capabilities"]["resources"].is_object());
        assert!(res["capabilities"]["prompts"].is_object());
        assert!(res["instructions"].as_str().unwrap().contains("triage"));
    }

    // Notifications get 202 with no body.
    let r = rpc_raw(
        &app,
        &tok,
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    assert!(r.text.is_empty());
    assert_eq!(
        rpc(&app, &tok, "ping", json!({})).await["result"],
        json!({})
    );
}

#[tokio::test]
async fn json_rpc_errors_and_batches() {
    let app = App::admin().await;
    let tok = token(&app, "write").await;

    let r = app
        .req(axum::http::Method::POST, "/mcp")
        .anonymous()
        .header("authorization", &format!("Bearer {tok}"))
        .header("content-type", "application/json")
        .send_raw("{not json")
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["error"]["code"], -32700);

    let r = rpc_raw(&app, &tok, json!([])).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["error"]["code"], -32600);

    let v = rpc(&app, &tok, "nope/nothing", json!({})).await;
    assert_eq!(v["error"]["code"], -32601);

    let v = rpc_raw(&app, &tok, json!({ "id": 1, "method": "ping" }))
        .await
        .json();
    assert_eq!(v["error"]["code"], -32600);

    let v = rpc_raw(
        &app,
        &tok,
        json!({ "jsonrpc": "2.0", "id": 1, "method": "ping", "params": [1] }),
    )
    .await
    .json();
    assert_eq!(v["error"]["code"], -32602);

    let v = rpc(&app, &tok, "tools/call", json!({ "name": "no_such_tool" })).await;
    assert_eq!(v["error"]["code"], -32602);
    let v = rpc(&app, &tok, "tools/call", json!({})).await;
    assert_eq!(v["error"]["code"], -32602);

    // Batch: requests answered in order, notifications skipped.
    let r = rpc_raw(
        &app,
        &tok,
        json!([
            { "jsonrpc": "2.0", "id": 1, "method": "ping" },
            { "jsonrpc": "2.0", "method": "notifications/initialized" },
            { "jsonrpc": "2.0", "id": 2, "method": "tools/list" },
            { "jsonrpc": "2.0", "id": 3, "method": "bogus" },
        ]),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let v = r.json();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0]["id"], 1);
    assert!(arr[1]["result"]["tools"].is_array());
    assert_eq!(arr[2]["error"]["code"], -32601);

    // Only notifications: 202.
    let r = rpc_raw(
        &app,
        &tok,
        json!([{ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 1 } }]),
    )
    .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn transport_rules() {
    let app = App::admin().await;
    let tok = token(&app, "read").await;
    let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });

    for method in [axum::http::Method::GET, axum::http::Method::DELETE] {
        let r = app
            .req(method, "/mcp")
            .anonymous()
            .header("authorization", &format!("Bearer {tok}"))
            .send()
            .await;
        assert_eq!(r.status, StatusCode::METHOD_NOT_ALLOWED);
    }

    let r = app
        .post("/mcp")
        .anonymous()
        .header("authorization", &format!("Bearer {tok}"))
        .header("origin", "https://evil.example")
        .json(ping.clone())
        .send()
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.json()["error"], "bad_origin");

    let r = app
        .post("/mcp")
        .anonymous()
        .header("authorization", &format!("Bearer {tok}"))
        .header("origin", "https://fh.test")
        .json(ping)
        .send()
        .await;
    assert_eq!(r.status, StatusCode::OK);
}

const WRITE_TOOLS: &[&str] = &[
    "create_label",
    "update_label",
    "delete_label",
    "label_feedback",
    "create_project",
    "update_project",
    "rotate_project_key",
    "delete_project",
    "create_feedback",
    "update_feedback",
    "bulk_update_feedback",
    "delete_feedback",
    "reindex_embeddings",
];
const READ_TOOLS: &[&str] = &[
    "list_labels",
    "list_projects",
    "get_project",
    "list_feedback",
    "search_feedback",
    "get_feedback",
    "find_similar_feedback",
    "get_stats",
    "export_feedback",
    "get_integration_snippets",
];

#[tokio::test]
async fn tools_list_is_scoped_and_schemas_are_complete() {
    let app = App::admin().await;
    let write = token(&app, "write").await;
    let read = token(&app, "read").await;

    let tools = rpc(&app, &write, "tools/list", json!({})).await["result"]["tools"].clone();
    let names: Vec<&str> = tools
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for t in READ_TOOLS.iter().chain(WRITE_TOOLS) {
        assert!(names.contains(t), "missing tool {t}");
    }
    assert_eq!(names.len(), READ_TOOLS.len() + WRITE_TOOLS.len());
    for t in tools.as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object");
        assert!(
            t["outputSchema"].is_object(),
            "{} lacks outputSchema",
            t["name"]
        );
        assert!(t["annotations"]["destructiveHint"].is_boolean());
    }
    let delete = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "delete_project")
        .unwrap();
    assert_eq!(delete["annotations"]["destructiveHint"], true);
    assert_eq!(
        delete["inputSchema"]["required"],
        json!(["project", "confirm_slug"])
    );

    let read_tools = rpc(&app, &read, "tools/list", json!({})).await["result"]["tools"].clone();
    let read_names: Vec<&str> = read_tools
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(read_names.len(), READ_TOOLS.len());
    assert!(WRITE_TOOLS.iter().all(|t| !read_names.contains(t)));

    // Read tokens get a recoverable tool error from write tools, and nothing changes.
    let msg = err(&app, &read, "create_project", json!({ "title": "Nope" })).await;
    assert!(msg.starts_with("forbidden"), "{msg}");
    let projects = ok(&app, &read, "list_projects", json!({})).await;
    assert_eq!(projects["projects"], json!([]));
}

#[tokio::test]
async fn project_tools_end_to_end() {
    let app = App::admin().await;
    let tok = token(&app, "write").await;

    let p = ok(
        &app,
        &tok,
        "create_project",
        json!({ "title": "Starfall", "project_type": "game", "description": "Space roguelike" }),
    )
    .await;
    assert_eq!(p["slug"], "starfall");
    let id = p["id"].as_i64().unwrap();
    let old_key = p["ingest_key"].as_str().unwrap().to_string();

    assert!(
        err(&app, &tok, "create_project", json!({}))
            .await
            .contains("title is required")
    );
    assert!(
        err(
            &app,
            &tok,
            "create_project",
            json!({ "title": "Dup", "slug": "starfall" })
        )
        .await
        .starts_with("conflict")
    );
    assert!(
        err(
            &app,
            &tok,
            "create_project",
            json!({ "title": "X", "project_type": "boat" })
        )
        .await
        .starts_with("invalid_project_type")
    );

    let list = ok(&app, &tok, "list_projects", json!({})).await;
    assert_eq!(list["projects"][0]["slug"], "starfall");
    assert!(list["projects"][0]["stats"].is_object());

    let by_slug = ok(&app, &tok, "get_project", json!({ "project": "starfall" })).await;
    let by_id = ok(
        &app,
        &tok,
        "get_project",
        json!({ "project": id.to_string() }),
    )
    .await;
    let by_num = ok(&app, &tok, "get_project", json!({ "project": id })).await;
    assert_eq!(by_slug["id"], id);
    assert_eq!(by_id["id"], id);
    assert_eq!(by_num["id"], id);
    assert!(
        err(&app, &tok, "get_project", json!({ "project": "nope" }))
            .await
            .starts_with("not_found")
    );

    let up = ok(
        &app,
        &tok,
        "update_project",
        json!({ "project": "starfall", "accepting": false, "description": "Paused" }),
    )
    .await;
    assert_eq!(up["accepting"], false);
    assert_eq!(up["description"], "Paused");
    assert_eq!(up["title"], "Starfall");

    let snip = ok(
        &app,
        &tok,
        "get_integration_snippets",
        json!({ "project": "starfall" }),
    )
    .await;
    assert_eq!(snip["hosted_url"], "https://fh.test/s/starfall");
    assert!(snip["widget_floating"].as_str().unwrap().contains(&old_key));
    assert!(snip["curl"].as_str().unwrap().contains("/api/v1/challenge"));
    let snip = ok(
        &app,
        &tok,
        "get_integration_snippets",
        json!({ "project": "starfall", "base_url": "https://fb.example.com/" }),
    )
    .await;
    assert_eq!(snip["hosted_url"], "https://fb.example.com/s/starfall");

    let rotated = ok(
        &app,
        &tok,
        "rotate_project_key",
        json!({ "project": "starfall" }),
    )
    .await;
    assert_ne!(rotated["ingest_key"], old_key.as_str());

    let msg = err(
        &app,
        &tok,
        "delete_project",
        json!({ "project": "starfall", "confirm_slug": "wrong" }),
    )
    .await;
    assert!(msg.starts_with("confirmation_required"), "{msg}");
    assert!(
        err(
            &app,
            &tok,
            "delete_project",
            json!({ "project": "starfall" })
        )
        .await
        .starts_with("confirmation_required")
    );
    let gone = ok(
        &app,
        &tok,
        "delete_project",
        json!({ "project": "starfall", "confirm_slug": "starfall" }),
    )
    .await;
    assert_eq!(gone["deleted"], true);
    assert_eq!(
        ok(&app, &tok, "list_projects", json!({})).await["projects"],
        json!([])
    );
}

#[tokio::test]
async fn feedback_tools_end_to_end() {
    let app = App::admin().await;
    let tok = token(&app, "write").await;
    ok(
        &app,
        &tok,
        "create_project",
        json!({ "title": "Starfall", "project_type": "game" }),
    )
    .await;
    ok(&app, &tok, "create_project", json!({ "title": "Website" })).await;

    let crash = ok(
        &app,
        &tok,
        "create_feedback",
        json!({
            "project": "starfall", "kind": "bug", "title": "Game crashes on boss fight",
            "body": "The game crashed and froze when the boss spawned. Version 1.2.3.",
            "meta": { "version": "1.2.3", "origin": "discord" }, "contact": "neo#1234"
        }),
    )
    .await;
    assert_eq!(crash["source"], "mcp");
    assert_eq!(crash["status"], "new");
    assert_eq!(crash["meta"]["origin"], "discord");
    let crash_id = crash["id"].as_i64().unwrap();
    let love = ok(
        &app,
        &tok,
        "create_feedback",
        json!({
            "project": "starfall", "kind": "feedback", "body": "I love the soundtrack and the art style",
            "rating": 5, "created_at": "2026-01-15"
        }),
    )
    .await;
    assert_eq!(love["title"], "I love the soundtrack and the art style");
    assert_eq!(love["created_at"], 1_768_435_200);
    let stuck = ok(
        &app,
        &tok,
        "create_feedback",
        json!({ "project": "starfall", "kind": "bug", "body": "Stuck on the loading screen, the game freezes forever" }),
    )
    .await;
    let web = ok(
        &app,
        &tok,
        "create_feedback",
        json!({ "project": "website", "kind": "bug", "body": "Login button does nothing on Safari" }),
    )
    .await;

    // Reuse check: MCP-created items are visible through the admin HTTP API.
    let via_http = app.feedback("status=any").await;
    assert_eq!(via_http["total"], 4);

    // Validation errors are tool errors with the admin API codes.
    assert!(
        err(
            &app,
            &tok,
            "create_feedback",
            json!({ "project": "starfall", "kind": "bug", "body": "x" })
        )
        .await
        .starts_with("invalid_body")
    );
    assert!(
        err(
            &app,
            &tok,
            "create_feedback",
            json!({ "project": "starfall", "kind": "idea", "body": "hello there" })
        )
        .await
        .starts_with("invalid_kind")
    );
    assert!(
        err(
            &app,
            &tok,
            "create_feedback",
            json!({ "project": "starfall", "kind": "bug", "body": "hello there", "meta": [1] })
        )
        .await
        .contains("meta")
    );
    assert!(
        err(
            &app,
            &tok,
            "create_feedback",
            json!({ "project": "ghost", "kind": "bug", "body": "hello there" })
        )
        .await
        .starts_with("not_found")
    );

    // list_feedback: filters, default open, pagination.
    let page = ok(
        &app,
        &tok,
        "list_feedback",
        json!({ "project": "starfall" }),
    )
    .await;
    assert_eq!(page["total"], 3);
    let page = ok(
        &app,
        &tok,
        "list_feedback",
        json!({ "kind": "bug", "limit": 1 }),
    )
    .await;
    assert_eq!(page["total"], 3);
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["next_offset"], 1);
    let page = ok(
        &app,
        &tok,
        "list_feedback",
        json!({ "kind": "bug", "limit": 1, "offset": 2 }),
    )
    .await;
    assert!(page["next_offset"].is_null());
    let page = ok(
        &app,
        &tok,
        "list_feedback",
        json!({ "project": "starfall", "to": "2026-01-15" }),
    )
    .await;
    assert_eq!(page["items"][0]["id"], love["id"]);
    assert_eq!(page["total"], 1);
    let page = ok(
        &app,
        &tok,
        "list_feedback",
        json!({ "project": "starfall", "days": 7 }),
    )
    .await;
    assert_eq!(page["total"], 2);
    assert!(
        err(
            &app,
            &tok,
            "list_feedback",
            json!({ "from": "last tuesday" })
        )
        .await
        .starts_with("invalid_arguments")
    );
    assert!(
        err(&app, &tok, "list_feedback", json!({ "status": "weird" }))
            .await
            .starts_with("invalid_query")
    );

    // Search modes, after the worker embeds everything.
    app.embed_all().await;
    assert!(
        err(&app, &tok, "search_feedback", json!({}))
            .await
            .contains("q is required")
    );
    let fuzzy = ok(
        &app,
        &tok,
        "search_feedback",
        json!({ "q": "sountrack", "mode": "fuzzy" }),
    )
    .await;
    assert_eq!(fuzzy["items"][0]["id"], love["id"]);
    let kw = ok(
        &app,
        &tok,
        "search_feedback",
        json!({ "q": "Safari", "mode": "keyword" }),
    )
    .await;
    assert_eq!(kw["items"][0]["id"], web["id"]);
    let sem = ok(
        &app,
        &tok,
        "search_feedback",
        json!({ "q": "hang", "mode": "semantic", "project": "starfall" }),
    )
    .await;
    assert_eq!(sem["semantic"], true);
    let sem_ids: Vec<i64> = sem["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_i64().unwrap())
        .collect();
    assert!(
        sem_ids.contains(&crash_id) || sem_ids.contains(&stuck["id"].as_i64().unwrap()),
        "{sem}"
    );
    assert!(!sem_ids.contains(&love["id"].as_i64().unwrap()));
    let hybrid = ok(&app, &tok, "search_feedback", json!({ "q": "boss crash" })).await;
    assert_eq!(hybrid["items"][0]["id"], crash_id);
    assert_eq!(hybrid["mode"], "hybrid");

    // get_feedback + similar.
    let detail = ok(&app, &tok, "get_feedback", json!({ "id": crash_id })).await;
    assert_eq!(detail["item"]["contact"], "neo#1234");
    assert!(detail["similar"].is_array());
    assert!(
        err(&app, &tok, "get_feedback", json!({ "id": 9999 }))
            .await
            .starts_with("not_found")
    );
    assert!(
        err(&app, &tok, "get_feedback", json!({}))
            .await
            .contains("id is required")
    );

    let sim = ok(
        &app,
        &tok,
        "find_similar_feedback",
        json!({ "id": crash_id }),
    )
    .await;
    assert_eq!(sim["semantic"], true);
    let sim_ids: Vec<i64> = sim["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_i64().unwrap())
        .collect();
    assert!(!sim_ids.contains(&crash_id), "must exclude itself");
    assert!(
        sim_ids.iter().all(|id| *id != web["id"].as_i64().unwrap()),
        "same project only"
    );
    assert_eq!(sim_ids.first(), stuck["id"].as_i64().as_ref());
    let sim = ok(
        &app,
        &tok,
        "find_similar_feedback",
        json!({ "text": "safari login broken", "limit": 1 }),
    )
    .await;
    assert_eq!(sim["items"][0]["id"], web["id"]);
    assert!(
        err(&app, &tok, "find_similar_feedback", json!({}))
            .await
            .starts_with("invalid_arguments")
    );

    // update_feedback with notes.
    assert!(
        err(&app, &tok, "update_feedback", json!({ "id": crash_id }))
            .await
            .contains("nothing to update")
    );
    let up = ok(
        &app,
        &tok,
        "update_feedback",
        json!({ "id": crash_id, "status": "triaged", "append_note": "severity major" }),
    )
    .await;
    assert_eq!(up["status"], "triaged");
    let up = ok(
        &app,
        &tok,
        "update_feedback",
        json!({ "id": crash_id, "append_note": "repro on 1.2.3" }),
    )
    .await;
    let note = up["admin_note"].as_str().unwrap();
    assert!(
        note.contains("severity major")
            && note.contains("repro on 1.2.3")
            && note.lines().count() == 2,
        "{note}"
    );
    assert!(
        err(
            &app,
            &tok,
            "update_feedback",
            json!({ "id": crash_id, "admin_note": "a", "append_note": "b" })
        )
        .await
        .starts_with("invalid_arguments")
    );
    assert!(
        err(
            &app,
            &tok,
            "update_feedback",
            json!({ "id": crash_id, "status": "done" })
        )
        .await
        .starts_with("invalid_status")
    );
    let up = ok(
        &app,
        &tok,
        "update_feedback",
        json!({ "id": crash_id, "kind": "feedback", "admin_note": "replaced" }),
    )
    .await;
    assert_eq!(up["kind"], "feedback");
    assert_eq!(up["admin_note"], "replaced");

    // Bulk.
    let ids = json!([love["id"], stuck["id"]]);
    let bulk = ok(
        &app,
        &tok,
        "bulk_update_feedback",
        json!({ "ids": ids, "status": "resolved" }),
    )
    .await;
    assert_eq!(bulk["affected"], 2);
    assert!(
        err(
            &app,
            &tok,
            "bulk_update_feedback",
            json!({ "ids": [], "status": "resolved" })
        )
        .await
        .starts_with("invalid_ids")
    );
    assert!(
        err(
            &app,
            &tok,
            "bulk_update_feedback",
            json!({ "ids": ["a"], "status": "resolved" })
        )
        .await
        .starts_with("invalid_arguments")
    );
    let resolved = ok(&app, &tok, "list_feedback", json!({ "status": "resolved" })).await;
    assert_eq!(resolved["total"], 2);

    // Stats.
    let stats = ok(&app, &tok, "get_stats", json!({ "project": "starfall" })).await;
    assert_eq!(stats["totals"]["total"], 3);
    assert_eq!(stats["daily"].as_array().unwrap().len(), 30);
    assert_eq!(stats["embeddings"]["pending"], 0);
    let all = ok(&app, &tok, "get_stats", json!({})).await;
    assert_eq!(all["totals"]["total"], 4);

    // Export.
    let ex = ok(
        &app,
        &tok,
        "export_feedback",
        json!({ "project": "starfall", "limit": 2 }),
    )
    .await;
    assert_eq!(ex["total"], 3);
    assert_eq!(ex["items"].as_array().unwrap().len(), 2);
    assert_eq!(ex["next_offset"], 2);
    assert_eq!(ex["items"][0]["id"], love["id"], "oldest first");
    let csv = ok(
        &app,
        &tok,
        "export_feedback",
        json!({ "project": "starfall", "format": "csv" }),
    )
    .await;
    let text = csv["csv"].as_str().unwrap();
    assert!(text.starts_with("id,kind,status"));
    assert_eq!(text.trim_end().lines().count(), 4);
    assert!(
        err(
            &app,
            &tok,
            "export_feedback",
            json!({ "project": "starfall", "format": "xml" })
        )
        .await
        .starts_with("invalid_arguments")
    );

    // Delete needs confirm.
    assert!(
        err(&app, &tok, "delete_feedback", json!({ "id": crash_id }))
            .await
            .starts_with("confirmation_required")
    );
    assert!(
        err(
            &app,
            &tok,
            "delete_feedback",
            json!({ "id": crash_id, "confirm": false })
        )
        .await
        .starts_with("confirmation_required")
    );
    let del = ok(
        &app,
        &tok,
        "delete_feedback",
        json!({ "id": crash_id, "confirm": true }),
    )
    .await;
    assert_eq!(del["deleted"], true);
    assert!(
        err(
            &app,
            &tok,
            "delete_feedback",
            json!({ "id": crash_id, "confirm": true })
        )
        .await
        .starts_with("not_found")
    );
    assert!(app.state.index.get(crash_id).is_none());

    // Reindex.
    let re = ok(&app, &tok, "reindex_embeddings", json!({})).await;
    assert_eq!(re["ok"], true);
    assert!(app.state.index.is_empty());
    app.embed_all().await;
    assert_eq!(app.state.index.len(), 3);
}

#[tokio::test]
async fn similar_falls_back_to_lexical_without_embeddings() {
    let mut app = App::with(|c| c.embed = feedback_harvest::config::EmbedConfig::Disabled).await;
    app.login().await;
    let tok = token(&app, "write").await;
    ok(&app, &tok, "create_project", json!({ "title": "Starfall" })).await;
    let a = ok(
        &app,
        &tok,
        "create_feedback",
        json!({ "project": "starfall", "kind": "bug", "body": "Inventory crashes when sorting" }),
    )
    .await;
    ok(&app, &tok, "create_feedback", json!({ "project": "starfall", "kind": "bug", "body": "Sorting the inventory crashes the game" })).await;
    let sim = ok(
        &app,
        &tok,
        "find_similar_feedback",
        json!({ "id": a["id"] }),
    )
    .await;
    assert_eq!(sim["semantic"], false);
    assert_eq!(sim["items"].as_array().unwrap().len(), 1);
    assert!(
        err(&app, &tok, "reindex_embeddings", json!({}))
            .await
            .starts_with("unavailable")
    );
}

#[tokio::test]
async fn resources_and_prompts() {
    let app = App::admin().await;
    let tok = token(&app, "read").await;
    let write = token(&app, "write").await;
    ok(
        &app,
        &write,
        "create_project",
        json!({ "title": "Starfall" }),
    )
    .await;
    let f = ok(
        &app,
        &write,
        "create_feedback",
        json!({ "project": "starfall", "kind": "bug", "body": "It crashed on launch" }),
    )
    .await;

    let list = rpc(&app, &tok, "resources/list", json!({})).await["result"]["resources"].clone();
    let uris: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["uri"].as_str().unwrap())
        .collect();
    assert_eq!(
        uris,
        vec![
            "feedback-harvest://projects",
            "feedback-harvest://projects/starfall"
        ]
    );

    let templates = rpc(&app, &tok, "resources/templates/list", json!({})).await;
    assert_eq!(
        templates["result"]["resourceTemplates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let read = |uri: String| {
        let app = &app;
        let tok = tok.clone();
        async move { rpc(app, &tok, "resources/read", json!({ "uri": uri })).await }
    };
    let v = read("feedback-harvest://projects".into()).await;
    let content = &v["result"]["contents"][0];
    assert_eq!(content["mimeType"], "application/json");
    let parsed: Value = serde_json::from_str(content["text"].as_str().unwrap()).unwrap();
    assert_eq!(parsed[0]["slug"], "starfall");

    let v = read("feedback-harvest://projects/starfall".into()).await;
    let parsed: Value =
        serde_json::from_str(v["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(parsed["open_total"], 1);
    assert_eq!(parsed["open_feedback"][0]["id"], f["id"]);

    let v = read(format!("feedback-harvest://feedback/{}", f["id"])).await;
    let parsed: Value =
        serde_json::from_str(v["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(parsed["item"]["body"], "It crashed on launch");

    for uri in [
        "feedback-harvest://projects/nope",
        "feedback-harvest://feedback/999",
        "feedback-harvest://feedback/abc",
        "other://x",
    ] {
        let v = read(uri.into()).await;
        assert_eq!(v["error"]["code"], -32002, "{uri}");
    }
    assert_eq!(
        rpc(&app, &tok, "resources/read", json!({})).await["error"]["code"],
        -32602
    );

    let prompts = rpc(&app, &tok, "prompts/list", json!({})).await["result"]["prompts"].clone();
    let names: Vec<&str> = prompts
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec![
            "group_feedback_themes",
            "triage_inbox",
            "summarize_feedback",
            "release_notes_from_resolved"
        ]
    );

    let p = rpc(&app, &tok, "prompts/get", json!({ "name": "triage_inbox" })).await;
    let text = p["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(text.contains("list_feedback") && text.contains("all projects"));
    let p = rpc(
        &app,
        &tok,
        "prompts/get",
        json!({ "name": "triage_inbox", "arguments": { "project": "starfall", "limit": "5" } }),
    )
    .await;
    assert!(
        p["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("limit=5")
    );
    let p = rpc(&app, &tok, "prompts/get", json!({ "name": "summarize_feedback", "arguments": { "project": "starfall", "days": "7" } })).await;
    assert!(
        p["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("days=7")
    );
    let p = rpc(&app, &tok, "prompts/get", json!({ "name": "release_notes_from_resolved", "arguments": { "project": "starfall", "since": "2026-10-01" } })).await;
    assert!(
        p["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("2026-10-01")
    );
    assert_eq!(
        rpc(
            &app,
            &tok,
            "prompts/get",
            json!({ "name": "summarize_feedback" })
        )
        .await["error"]["code"],
        -32602
    );
    assert_eq!(
        rpc(&app, &tok, "prompts/get", json!({ "name": "nope" })).await["error"]["code"],
        -32602
    );
}

#[tokio::test]
async fn imported_feedback_is_never_flagged_as_near_duplicate() {
    let app = App::admin().await;
    let tok = token(&app, "write").await;
    ok(&app, &tok, "create_project", json!({ "title": "Starfall" })).await;
    // Two players on Discord report the same thing; the agent imports both.
    for suffix in ["", "!"] {
        ok(
            &app,
            &tok,
            "create_feedback",
            json!({
                "project": "starfall", "kind": "bug",
                "body": format!("The inventory crashes when I open it during combat in level three{suffix}")
            }),
        )
        .await;
    }
    app.embed_all().await;
    assert_eq!(app.feedback("status=spam").await["total"], 0);
    assert_eq!(app.feedback("").await["total"], 2);
}

#[tokio::test]
async fn labels_tools_crud_filtering_scopes_and_theme_prompt() {
    let app = App::admin().await;
    let write = token(&app, "write").await;
    let read = token(&app, "read").await;
    let p = ok(&app, &write, "create_project", json!({"title":"Themes"})).await;
    let f=ok(&app,&write,"create_feedback",json!({"project":"themes","kind":"bug","body":"The game crashes when saving inventory","title":"Save crash"})).await;
    let l=ok(&app,&write,"create_label",json!({"project":"themes","name":"Save integrity","description":"Corrupt saves and crashes while saving","color":"red"})).await;
    let lid = l["id"].as_i64().unwrap();
    assert!(
        err(
            &app,
            &write,
            "create_label",
            json!({"project":"themes","name":"SAVE INTEGRITY"})
        )
        .await
        .starts_with("conflict:")
    );
    assert!(
        err(
            &app,
            &write,
            "create_label",
            json!({"project":"themes","name":"bad","color":"nope"})
        )
        .await
        .starts_with("invalid_labels:")
    );
    for (name, args) in [
        ("create_label", json!({"project":"themes","name":"x"})),
        ("update_label", json!({"id":lid,"name":"x"})),
        ("delete_label", json!({"id":lid,"confirm":true})),
        (
            "label_feedback",
            json!({"ids":[f["id"]],"label_ids":[lid],"action":"add"}),
        ),
    ] {
        assert!(err(&app, &read, name, args).await.starts_with("forbidden:"));
    }
    assert!(
        err(
            &app,
            &write,
            "label_feedback",
            json!({"ids":[f["id"]],"label_ids":["bad"],"action":"add"})
        )
        .await
        .starts_with("invalid_arguments:")
    );
    assert_eq!(
        ok(
            &app,
            &write,
            "label_feedback",
            json!({"ids":[f["id"]],"label_ids":[lid],"action":"add"})
        )
        .await["affected"],
        1
    );
    assert_eq!(
        ok(
            &app,
            &write,
            "label_feedback",
            json!({"ids":[f["id"]],"label_ids":[lid],"action":"add"})
        )
        .await["affected"],
        0
    );
    let list = ok(&app, &read, "list_labels", json!({"project":p["slug"]})).await;
    assert_eq!(list["labels"][0]["open_count"], 1);
    assert_eq!(
        ok(&app, &read, "list_feedback", json!({"label_ids":[lid]})).await["total"],
        1
    );
    assert_eq!(
        ok(
            &app,
            &read,
            "search_feedback",
            json!({"q":"crash","mode":"keyword","label_ids":[lid],"label_match":"all"})
        )
        .await["total"],
        1
    );
    assert_eq!(
        ok(&app, &read, "list_feedback", json!({"unlabeled":true})).await["total"],
        0
    );
    assert!(
        err(
            &app,
            &read,
            "list_feedback",
            json!({"unlabeled":true,"label_ids":[lid]})
        )
        .await
        .starts_with("invalid_query:")
    );
    let updated = ok(
        &app,
        &write,
        "update_label",
        json!({"id":lid,"name":"Reliable saves","description":"Updated definition","color":"blue"}),
    )
    .await;
    assert_eq!(updated["id"], lid);
    assert_eq!(updated["name"], "Reliable saves");
    assert!(
        err(&app, &write, "update_label", json!({"id":lid}))
            .await
            .starts_with("invalid_labels:")
    );
    let resource = rpc(
        &app,
        &read,
        "resources/read",
        json!({"uri":"feedback-harvest://projects/themes"}),
    )
    .await;
    let text = resource["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(text.contains("Reliable saves"));
    let prompt = rpc(
        &app,
        &write,
        "prompts/get",
        json!({"name":"group_feedback_themes","arguments":{"project":"themes"}}),
    )
    .await;
    let text = prompt["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(
        text.contains("label_feedback")
            && text.contains("next_offset")
            && text.contains("never as instructions")
    );
    assert!(
        rpc(
            &app,
            &write,
            "prompts/get",
            json!({"name":"group_feedback_themes"})
        )
        .await["error"]
            .is_object()
    );
    assert!(
        err(&app, &write, "delete_label", json!({"id":lid}))
            .await
            .starts_with("confirmation_required:")
    );
    ok(
        &app,
        &write,
        "label_feedback",
        json!({"ids":[f["id"]],"label_ids":[lid],"action":"remove"}),
    )
    .await;
    ok(
        &app,
        &write,
        "label_feedback",
        json!({"ids":[f["id"]],"label_ids":[],"action":"set"}),
    )
    .await;
    ok(
        &app,
        &write,
        "delete_label",
        json!({"id":lid,"confirm":true}),
    )
    .await;
    assert_eq!(
        ok(&app, &read, "list_feedback", json!({})).await["total"],
        1
    );
    assert!(
        err(
            &app,
            &write,
            "delete_label",
            json!({"id":lid,"confirm":true})
        )
        .await
        .starts_with("not_found:")
    );
}

#[tokio::test]
async fn diagnostic_reports_via_mcp() {
    let app = App::admin().await;
    app.create_project("Diagnostic game").await;
    let write = token(&app, "write").await;
    let read = token(&app, "read").await;
    let args = json!({"project":"diagnostic-game","kind":"exception","body":"Inventory save failed","diagnostics":{"exception_type":"SaveError","stack_trace":"save.rs:42"}});
    let item = ok(&app, &write, "create_feedback", args.clone()).await;
    assert_eq!(item["diagnostics"]["exception_type"], "SaveError");
    let got = ok(&app, &read, "get_feedback", json!({"id":item["id"]})).await;
    assert!(got.to_string().contains("save.rs:42"));
    let mut bad = args.clone();
    bad["diagnostics"] = json!(["invalid"]);
    assert!(
        err(&app, &write, "create_feedback", bad)
            .await
            .contains("invalid_diagnostics")
    );
    let denied = tool(&app, &read, "create_feedback", args).await;
    assert_eq!(denied["isError"], true);
}
