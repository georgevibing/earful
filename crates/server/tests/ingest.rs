mod common;

use axum::http::StatusCode;
use common::{App, ids, key_of};
use earful::{crypto, db::now_ms, pow};
use serde_json::{Value, json};

async fn setup() -> (App, String, i64) {
    let app = App::admin().await;
    let p = app.create_project("Space Game").await;
    (app, key_of(&p), p["id"].as_i64().unwrap())
}

fn valid() -> Value {
    json!({ "kind": "bug", "title": "Crash on boss", "body": "Game crashes when the boss spawns." })
}

#[tokio::test]
async fn happy_path_submission_shows_up_in_admin() {
    let (app, key, pid) = setup().await;
    let r = app
        .submit(
            &key,
            "2.2.2.2",
            json!({
                "kind": "bug", "title": "Crash on boss", "body": "Game crashes when the boss spawns.",
                "contact": "me@example.com", "rating": 2, "meta": { "version": "1.2.3", "os": "linux" }, "source": "web",
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    assert_eq!(r.json(), json!({ "ok": true }));

    let list = app.feedback(&format!("project_id={pid}")).await;
    assert_eq!(list["total"], 1);
    let item = &list["items"][0];
    assert_eq!(item["kind"], "bug");
    assert_eq!(item["status"], "new");
    assert_eq!(item["contact"], "me@example.com");
    assert_eq!(item["rating"], 2);
    assert_eq!(item["meta"]["version"], "1.2.3");
    assert_eq!(item["source"], "web");
    assert!(item.get("ip_hash").is_none(), "ip hash must not leak");
    assert!(item.get("score").is_none());
}

#[tokio::test]
async fn key_can_come_from_header_and_title_is_derived() {
    let (app, key, _) = setup().await;
    let ch = app.challenge(&key, "2.2.2.2").await.json();
    let token = ch["challenge"].as_str().unwrap();
    let nonce = pow::solve(token, ch["bits"].as_u64().unwrap() as u32);
    let r = app
        .post("/api/v1/feedback")
        .anonymous()
        .ip("2.2.2.2")
        .header("x-earful-key", &key)
        .json(json!({ "kind": "feedback", "body": "\n  Love the music\nand the art", "challenge": token, "nonce": nonce.parse::<u64>().unwrap() }))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.text);
    let list = app.feedback("").await;
    assert_eq!(list["items"][0]["title"], "Love the music");
}

#[tokio::test]
async fn pre_rename_key_header_is_still_accepted() {
    let (app, key, _) = setup().await;
    let ch = app.challenge(&key, "2.2.2.3").await.json();
    let token = ch["challenge"].as_str().unwrap();
    let nonce = pow::solve(token, ch["bits"].as_u64().unwrap() as u32);
    let r = app
        .post("/api/v1/feedback")
        .anonymous()
        .ip("2.2.2.3")
        .header("x-fh-key", &key)
        .json(json!({ "kind": "bug", "body": "Legacy header still works", "challenge": token, "nonce": nonce.parse::<u64>().unwrap() }))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.text);
}

#[tokio::test]
async fn unknown_key_and_paused_project() {
    let (app, key, pid) = setup().await;
    assert_eq!(
        app.challenge("ear_nope", "2.2.2.2").await.status,
        StatusCode::NOT_FOUND
    );
    let r = app.get("/api/v1/challenge").anonymous().send().await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    let ch = app.challenge(&key, "2.2.2.2").await.json();
    app.patch(&format!("/api/admin/projects/{pid}"))
        .json(json!({ "accepting": false }))
        .send()
        .await;
    let r = app.challenge(&key, "2.2.2.2").await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.json()["error"], "project_paused");
    // A challenge obtained before pausing cannot be redeemed either.
    let token = ch["challenge"].as_str().unwrap();
    let mut body = valid();
    body["key"] = json!(key);
    body["challenge"] = json!(token);
    body["nonce"] = json!(pow::solve(token, ch["bits"].as_u64().unwrap() as u32));
    let r = app
        .post("/api/v1/feedback")
        .anonymous()
        .json(body)
        .send()
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn honeypot_is_silently_dropped() {
    let (app, key, _) = setup().await;
    let mut body = valid();
    body["website"] = json!("http://spam.example");
    let r = app.submit(&key, "2.2.2.2", body).await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    assert_eq!(app.feedback("status=any").await["total"], 0);
    let stats = app.get("/api/admin/stats").send().await.json();
    assert_eq!(stats["blocked"]["honeypot"], 1);
}

#[tokio::test]
async fn validation_errors_are_specific() {
    let (app, key, _) = setup().await;
    let cases = [
        (
            json!({ "kind": "rant", "body": "hello there" }),
            "invalid_kind",
        ),
        (json!({ "body": "hello there" }), "invalid_kind"),
        (json!({ "kind": "bug", "body": "hi" }), "invalid_body"),
        (
            json!({ "kind": "bug", "body": "x".repeat(10_001) }),
            "invalid_body",
        ),
        (
            json!({ "kind": "bug", "body": "hello", "title": "t".repeat(201) }),
            "invalid_title",
        ),
        (
            json!({ "kind": "bug", "body": "hello", "contact": "c".repeat(201) }),
            "invalid_contact",
        ),
        (
            json!({ "kind": "bug", "body": "hello", "rating": 6 }),
            "invalid_rating",
        ),
        (
            json!({ "kind": "bug", "body": "hello", "meta": [1, 2] }),
            "invalid_meta",
        ),
        (
            json!({ "kind": "bug", "body": "hello", "meta": { "x": "y".repeat(5000) } }),
            "invalid_meta",
        ),
        (
            json!({ "kind": "bug", "body": "hello", "source": "mail" }),
            "invalid_source",
        ),
    ];
    for (i, (mut body, code)) in cases.into_iter().enumerate() {
        body["key"] = json!(key);
        let r = app
            .post("/api/v1/feedback")
            .anonymous()
            .ip(&format!("3.3.3.{i}"))
            .json(body)
            .send()
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{code}: {}", r.text);
        assert_eq!(r.json()["error"], code);
    }
    let r = app
        .post("/api/v1/feedback")
        .anonymous()
        .header("content-type", "application/json")
        .send()
        .await;
    assert_eq!(r.json()["error"], "invalid_json");
}

#[tokio::test]
async fn oversized_bodies_are_rejected() {
    let (app, key, _) = setup().await;
    let body = json!({ "key": key, "kind": "bug", "body": "x".repeat(140_000) });
    let r = app
        .post("/api/v1/feedback")
        .anonymous()
        .json(body)
        .send()
        .await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn per_ip_rate_limit_with_retry_after() {
    let (app, key, _) = setup().await;
    for i in 0..3 {
        app.submit_ok(
            &key,
            "4.4.4.4",
            "bug",
            &format!("t{i}"),
            &format!("distinct body number {i}"),
        )
        .await;
    }
    let r = app
        .submit(
            &key,
            "4.4.4.4",
            json!({ "kind": "bug", "body": "fourth body here" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(r.json()["error"], "rate_limited");
    let retry: i64 = r.header("retry-after").unwrap().parse().unwrap();
    assert!((1..=60).contains(&retry), "{retry}");
    // Someone else is fine.
    app.submit_ok(&key, "4.4.4.5", "bug", "other", "other person body")
        .await;
    let stats = app.get("/api/admin/stats").send().await.json();
    assert_eq!(stats["blocked"]["rate_limited"], 1);
}

#[tokio::test]
async fn per_project_hourly_cap_spans_ips() {
    let app = App::with(|c| c.limits.per_project_hour = 3).await;
    let mut app = app;
    app.login().await;
    let p = app.create_project("Busy").await;
    let key = key_of(&p);
    for i in 0..3 {
        app.submit_ok(
            &key,
            &format!("8.8.8.{i}"),
            "feedback",
            "",
            &format!("body from ip {i}"),
        )
        .await;
    }
    let r = app
        .submit(
            &key,
            "8.8.8.200",
            json!({ "kind": "bug", "body": "late body" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn global_per_ip_daily_cap_spans_projects() {
    let mut app = App::with(|c| c.limits.per_ip_global_day = 2).await;
    app.login().await;
    let a = key_of(&app.create_project("A").await);
    let b = key_of(&app.create_project("B").await);
    app.submit_ok(&a, "4.3.2.1", "bug", "", "first body").await;
    app.submit_ok(&b, "4.3.2.1", "bug", "", "second body").await;
    let r = app
        .submit(
            &b,
            "4.3.2.1",
            json!({ "kind": "bug", "body": "third body" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn challenge_issuing_is_rate_limited() {
    let mut app = App::with(|c| c.limits.challenges_per_ip_hour = 2).await;
    app.login().await;
    let key = key_of(&app.create_project("A").await);
    assert_eq!(app.challenge(&key, "1.2.3.4").await.status, StatusCode::OK);
    assert_eq!(app.challenge(&key, "1.2.3.4").await.status, StatusCode::OK);
    let r = app.challenge(&key, "1.2.3.4").await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(r.header("retry-after").is_some());
}

#[tokio::test]
async fn challenge_response_shape() {
    let (app, key, _) = setup().await;
    let r = app.challenge(&key, "2.2.2.2").await;
    assert_eq!(r.header("cache-control"), Some("no-store"));
    let j = r.json();
    assert_eq!(j["bits"], 4);
    assert_eq!(j["expires_in"], 600);
    assert!(j["challenge"].as_str().unwrap().starts_with("v1."));
}

#[tokio::test]
async fn pow_failures_are_rejected_with_codes() {
    let (app, key, pid) = setup().await;
    let other = key_of(&app.create_project("Other").await);
    let secret = app.state.secret.clone();

    let post = |challenge: String, nonce: String| {
        let mut body = valid();
        body["key"] = json!(key);
        body["challenge"] = json!(challenge);
        body["nonce"] = json!(nonce);
        app.post("/api/v1/feedback")
            .anonymous()
            .ip("2.2.2.2")
            .json(body)
            .send()
    };

    // Missing challenge.
    let r = post(String::new(), "1".into()).await;
    assert_eq!(r.json()["error"], "challenge_malformed");

    // Wrong nonce.
    let token = app.challenge(&key, "2.2.2.2").await.json()["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    let bad = (0u64..)
        .map(|n| n.to_string())
        .find(|n| pow::work_bits(&token, n) < 4)
        .unwrap();
    let r = post(token.clone(), bad).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["error"], "pow_insufficient");

    // Forged signature.
    let forged = format!(
        "{}{}",
        &token[..token.len() - 1],
        if token.ends_with('0') { '1' } else { '0' }
    );
    let r = post(forged.clone(), pow::solve(&forged, 4)).await;
    assert_eq!(r.json()["error"], "challenge_invalid");

    // Challenge for another project.
    let foreign = app.challenge(&other, "2.2.2.2").await.json()["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    let r = post(foreign.clone(), pow::solve(&foreign, 4)).await;
    assert_eq!(r.json()["error"], "challenge_wrong_project");

    // Expired.
    let old = pow::sign(&secret, pid, now_ms() - 700_000, 4, &crypto::random_hex(16));
    let r = post(old.clone(), pow::solve(&old, 4)).await;
    assert_eq!(r.json()["error"], "challenge_expired");

    // Replay: first use succeeds, second is rejected.
    let nonce = pow::solve(&token, 4);
    assert_eq!(
        post(token.clone(), nonce.clone()).await.status,
        StatusCode::ACCEPTED
    );
    let r = post(token, nonce).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["error"], "challenge_used");

    let blocked = &app.get("/api/admin/stats").send().await.json()["blocked"];
    assert_eq!(blocked["pow_insufficient"], 1);
    assert_eq!(blocked["challenge_used"], 1);
    assert_eq!(blocked["challenge_expired"], 1);
    assert_eq!(app.feedback("").await["total"], 1);
}

#[tokio::test]
async fn difficulty_adapts_to_repeat_senders() {
    let (app, key, _) = setup().await;
    let bits = |ip: &'static str| {
        let app = &app;
        let key = key.clone();
        async move {
            app.challenge(&key, ip).await.json()["bits"]
                .as_u64()
                .unwrap()
        }
    };
    assert_eq!(bits("2.2.2.2").await, 4);
    app.submit_ok(&key, "2.2.2.2", "bug", "", "first distinct body")
        .await;
    assert_eq!(bits("2.2.2.2").await, 5);
    app.submit_ok(&key, "2.2.2.2", "bug", "", "second distinct body")
        .await;
    assert_eq!(bits("2.2.2.2").await, 6);
    assert_eq!(bits("2.2.2.3").await, 4, "other senders are unaffected");
}

#[tokio::test]
async fn difficulty_is_clamped_to_max() {
    let mut app = App::with(|c| {
        c.pow.base_bits = 4;
        c.pow.max_bits = 5;
        c.limits.per_ip_project_minute = 100;
        c.limits.per_ip_project_hour = 100;
    })
    .await;
    app.login().await;
    let key = key_of(&app.create_project("A").await);
    for i in 0..3 {
        app.submit_ok(&key, "2.2.2.2", "bug", "", &format!("body {i} words"))
            .await;
    }
    assert_eq!(app.challenge(&key, "2.2.2.2").await.json()["bits"], 5);
}

#[tokio::test]
async fn exact_duplicates_merge_or_drop() {
    let (app, key, _) = setup().await;
    app.submit_ok(&key, "2.2.2.2", "bug", "Crash", "The game crashes on start")
        .await;
    // Same text, other user: counted on the original.
    app.submit_ok(
        &key,
        "2.2.2.3",
        "bug",
        "crash",
        "the  game crashes on START",
    )
    .await;
    // Same text, same user: dropped.
    app.submit_ok(&key, "2.2.2.2", "bug", "Crash", "The game crashes on start")
        .await;
    let list = app.feedback("status=any").await;
    assert_eq!(list["total"], 1);
    assert_eq!(list["items"][0]["dup_count"], 1);
    let stats = app.get("/api/admin/stats").send().await.json();
    assert_eq!(stats["blocked"]["duplicate"], 1);
}

#[tokio::test]
async fn heuristic_spam_is_quarantined() {
    let (app, key, _) = setup().await;
    app.submit_ok(
        &key,
        "2.2.2.2",
        "feedback",
        "Best casino",
        "Visit https://a.example https://b.example https://bit.ly/x for free bitcoin",
    )
    .await;
    assert_eq!(
        app.feedback("").await["total"],
        0,
        "spam is not in the inbox"
    );
    let spam = app.feedback("status=spam").await;
    assert_eq!(spam["total"], 1);
    let reason = spam["items"][0]["spam_reason"].as_str().unwrap();
    assert!(reason.starts_with("spam_score"), "{reason}");
    assert!(spam["items"][0]["spam_score"].as_f64().unwrap() >= 0.7);
    assert_eq!(
        app.get("/api/admin/stats").send().await.json()["blocked"]["spam_score"],
        1
    );
}

#[tokio::test]
async fn too_fast_submissions_are_quarantined() {
    let mut app = App::with(|c| c.limits.min_fill_secs = 60).await;
    app.login().await;
    let key = key_of(&app.create_project("A").await);
    app.submit_ok(&key, "2.2.2.2", "bug", "", "a normal looking bug report")
        .await;
    let spam = app.feedback("status=spam").await;
    assert_eq!(spam["items"][0]["spam_reason"], "too_fast");
    assert_eq!(
        app.get("/api/admin/stats").send().await.json()["blocked"]["too_fast"],
        1
    );
}

#[tokio::test]
async fn near_duplicates_from_one_ip_are_flagged_after_embedding() {
    let (app, key, _) = setup().await;
    app.submit_ok(
        &key,
        "2.2.2.2",
        "bug",
        "Inventory",
        "The inventory crashes when I open it during combat in level three",
    )
    .await;
    app.submit_ok(
        &key,
        "2.2.2.2",
        "bug",
        "Inventory",
        "The inventory crashes when I open it during combat in level three!!",
    )
    .await;
    app.submit_ok(
        &key,
        "2.2.2.9",
        "bug",
        "Inventory",
        "The inventory crashes when I open it during combat in level three?",
    )
    .await;
    app.embed_all().await;
    let open = app.feedback("").await;
    let spam = app.feedback("status=spam").await;
    assert_eq!(open["total"], 2, "{open}");
    assert_eq!(spam["total"], 1);
    assert_eq!(spam["items"][0]["spam_reason"], "near_duplicate");
    assert_eq!(
        app.get("/api/admin/stats").send().await.json()["blocked"]["near_duplicate"],
        1
    );
}

#[tokio::test]
async fn control_characters_are_stripped() {
    let (app, key, _) = setup().await;
    app.submit_ok(&key, "2.2.2.2", "bug", "Ti\u{0}tle", "bo\u{7}dy text")
        .await;
    let item = &app.feedback("").await["items"][0];
    assert_eq!(item["title"], "Title");
    assert_eq!(item["body"], "body text");
}

#[tokio::test]
async fn submissions_are_isolated_per_project() {
    let (app, key, pid) = setup().await;
    let other = app.create_project("Other").await;
    app.submit_ok(&key, "2.2.2.2", "bug", "", "space game bug")
        .await;
    app.submit_ok(&key_of(&other), "2.2.2.2", "bug", "", "other game bug")
        .await;
    assert_eq!(
        ids(&app.feedback(&format!("project_id={pid}")).await).len(),
        1
    );
}

#[tokio::test]
async fn admin_restored_items_are_not_requarantined_by_a_reindex() {
    let (app, key, _) = setup().await;
    let text = "The inventory crashes when I open it during combat in level three";
    app.submit_ok(&key, "2.2.2.2", "bug", "Inventory", text)
        .await;
    app.submit_ok(&key, "2.2.2.2", "bug", "Inventory", &format!("{text}!!"))
        .await;
    app.embed_all().await;
    let spam = app.feedback("status=spam").await;
    let id = ids(&spam)[0];
    // The admin decides it is not spam after all.
    let r = app
        .patch(&format!("/api/admin/feedback/{id}"))
        .json(json!({ "status": "new" }))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(
        app.post("/api/admin/reindex").send().await.status,
        StatusCode::ACCEPTED
    );
    app.embed_all().await;
    assert_eq!(app.feedback("status=spam").await["total"], 0);
    assert_eq!(app.feedback("").await["total"], 2);
}

#[tokio::test]
async fn blocked_requests_do_not_grow_the_database_per_request() {
    let (app, key, _) = setup().await;
    for _ in 0..40 {
        let r = app
            .post("/api/v1/feedback")
            .anonymous()
            .ip("3.3.3.3")
            .json(json!({ "key": key, "kind": "bug", "body": "spam spam", "website": "x" }))
            .send()
            .await;
        assert_eq!(r.status, StatusCode::ACCEPTED);
    }
    let stats = app.get("/api/admin/stats").send().await.json();
    assert_eq!(stats["blocked"]["honeypot"], 40);
    // Blocks are aggregated per day, project and reason: one row, not forty.
    let rows: i64 = app
        .state
        .db
        .call(|c| Ok(c.query_row("SELECT COUNT(*) FROM block_counts", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert_eq!(rows, 1);
}
