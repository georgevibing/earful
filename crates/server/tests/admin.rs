mod common;

use axum::http::StatusCode;
use common::{App, ids, key_of, titles};
use serde_json::json;

// ---------------------------------------------------------------------------
// Projects

#[tokio::test]
async fn project_crud_and_validation() {
    let app = App::admin().await;
    let p = app
        .post("/api/admin/projects")
        .json(json!({ "title": "My Cool Game!", "project_type": "game", "description": "A roguelike" }))
        .send()
        .await;
    assert_eq!(p.status, StatusCode::CREATED);
    let p = p.json();
    assert_eq!(p["slug"], "my-cool-game");
    assert_eq!(p["project_type"], "game");
    assert_eq!(p["accepting"], true);
    assert!(p["ingest_key"].as_str().unwrap().starts_with("fhk_"));
    assert_eq!(p["ingest_key"].as_str().unwrap().len(), 36);
    assert_eq!(p["stats"]["total"], 0);
    let id = p["id"].as_i64().unwrap();

    for (body, code) in [
        (json!({ "title": "" }), "invalid_title"),
        (json!({ "title": "x".repeat(121) }), "invalid_title"),
        (json!({ "title": "X", "slug": "Bad Slug" }), "invalid_slug"),
        (
            json!({ "title": "X", "project_type": "toaster" }),
            "invalid_project_type",
        ),
        (
            json!({ "title": "X", "description": "d".repeat(2001) }),
            "invalid_description",
        ),
    ] {
        let r = app.post("/api/admin/projects").json(body).send().await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{code}");
        assert_eq!(r.json()["error"], code);
    }

    let dup = app
        .post("/api/admin/projects")
        .json(json!({ "title": "Other", "slug": "my-cool-game" }))
        .send()
        .await;
    assert_eq!(dup.status, StatusCode::CONFLICT);

    let unicode = app.create_project("日本語").await;
    assert!(unicode["slug"].as_str().unwrap().starts_with("project-"));

    let r = app
        .patch(&format!("/api/admin/projects/{id}"))
        .json(json!({ "title": "Renamed", "description": "new", "project_type": "webapp" }))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = r.json();
    assert_eq!(r["title"], "Renamed");
    assert_eq!(r["slug"], "my-cool-game", "slug does not follow title");
    assert_eq!(r["project_type"], "webapp");

    let conflict = app
        .patch(&format!("/api/admin/projects/{}", unicode["id"]))
        .json(json!({ "slug": "my-cool-game" }))
        .send()
        .await;
    assert_eq!(conflict.status, StatusCode::CONFLICT);

    let list = app.get("/api/admin/projects").send().await.json();
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert_eq!(
        app.get(&format!("/api/admin/projects/{id}"))
            .send()
            .await
            .json()["title"],
        "Renamed"
    );
    assert_eq!(
        app.get("/api/admin/projects/999").send().await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.get("/api/admin/projects/abc").send().await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.patch("/api/admin/projects/999")
            .json(json!({ "title": "x" }))
            .send()
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.delete("/api/admin/projects/999").send().await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn project_stats_count_feedback() {
    let app = App::admin().await;
    let key = key_of(&app.create_project("A").await);
    app.submit_ok(&key, "2.2.2.1", "bug", "", "first bug body")
        .await;
    app.submit_ok(&key, "2.2.2.2", "feedback", "", "some nice feedback")
        .await;
    app.submit_ok(
        &key,
        "2.2.2.3",
        "feedback",
        "casino",
        "free bitcoin casino https://a.b https://c.d https://e.f",
    )
    .await;
    let p = &app.get("/api/admin/projects").send().await.json()[0];
    assert_eq!(p["stats"]["total"], 2);
    assert_eq!(p["stats"]["open"], 2);
    assert_eq!(p["stats"]["new"], 2);
    assert_eq!(p["stats"]["bugs"], 1);
    assert_eq!(p["stats"]["feedback"], 1);
    assert_eq!(p["stats"]["spam"], 1);
    assert!(p["stats"]["last_at"].as_i64().unwrap() > 0);
}

#[tokio::test]
async fn rotating_the_key_revokes_the_old_one() {
    let app = App::admin().await;
    let p = app.create_project("A").await;
    let old = key_of(&p);
    let r = app
        .post(&format!("/api/admin/projects/{}/rotate-key", p["id"]))
        .send()
        .await
        .json();
    let new = key_of(&r);
    assert_ne!(old, new);
    assert_eq!(
        app.challenge(&old, "2.2.2.2").await.status,
        StatusCode::NOT_FOUND
    );
    app.submit_ok(&new, "2.2.2.2", "bug", "", "works with the new key")
        .await;
}

#[tokio::test]
async fn deleting_a_project_cascades_everywhere() {
    let app = App::admin().await;
    let p = app.create_project("Doomed").await;
    let keep = app.create_project("Keep").await;
    let key = key_of(&p);
    app.submit_ok(
        &key,
        "2.2.2.2",
        "bug",
        "Inventory crash",
        "inventory crashes all the time",
    )
    .await;
    app.submit_ok(&key_of(&keep), "2.2.2.3", "bug", "Audio", "sound cuts out")
        .await;
    app.embed_all().await;
    assert_eq!(app.state.index.len(), 2);

    assert_eq!(
        app.delete(&format!("/api/admin/projects/{}", p["id"]))
            .send()
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(app.state.index.len(), 1);
    assert_eq!(app.sql_i64("SELECT COUNT(*) FROM feedback").await, 1);
    assert_eq!(app.sql_i64("SELECT COUNT(*) FROM embeddings").await, 1);
    for mode in ["keyword", "fuzzy", "semantic", "hybrid"] {
        let r = app
            .feedback(&format!("q=inventory&mode={mode}&status=any"))
            .await;
        assert_eq!(r["total"], 0, "{mode}: {r}");
    }
    // External content FTS tables must still match the base table.
    app.state
        .db
        .call(|c| {
            c.execute(
                "INSERT INTO feedback_fts (feedback_fts) VALUES ('integrity-check')",
                [],
            )?;
            c.execute(
                "INSERT INTO feedback_tri (feedback_tri) VALUES ('integrity-check')",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        app.challenge(&key, "2.2.2.2").await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn export_json_and_csv() {
    let app = App::admin().await;
    let p = app.create_project("Export Me").await;
    let key = key_of(&p);
    app.submit_ok(
        &key,
        "2.2.2.2",
        "bug",
        "Comma, \"quote\"",
        "line one\nline two",
    )
    .await;
    app.submit_ok(&key, "2.2.2.3", "feedback", "", "=HYPERLINK(\"http://x\")")
        .await;
    let id = p["id"].as_i64().unwrap();

    let json_resp = app
        .get(&format!("/api/admin/projects/{id}/export"))
        .send()
        .await;
    assert_eq!(json_resp.status, StatusCode::OK);
    assert_eq!(json_resp.header("content-type"), Some("application/json"));
    assert_eq!(
        json_resp.header("content-disposition"),
        Some("attachment; filename=\"export-me-feedback.json\"")
    );
    assert_eq!(json_resp.json().as_array().unwrap().len(), 2);

    let csv = app
        .get(&format!("/api/admin/projects/{id}/export?format=csv"))
        .send()
        .await;
    assert_eq!(csv.header("content-type"), Some("text/csv; charset=utf-8"));
    assert!(
        csv.header("content-disposition")
            .unwrap()
            .contains("export-me-feedback.csv")
    );
    assert!(csv.text.starts_with("id,kind,status,title,body,"));
    assert!(
        csv.text.contains("\"Comma, \"\"quote\"\"\""),
        "{}",
        csv.text
    );
    assert!(csv.text.contains("\"line one\nline two\""));
    assert!(
        csv.text.contains("\"'=HYPERLINK(\"\"http://x\"\")\""),
        "{}",
        csv.text
    );
    assert_eq!(csv.text.matches("\r\n").count(), 3);

    let bad = app
        .get(&format!("/api/admin/projects/{id}/export?format=xml"))
        .send()
        .await;
    assert_eq!(bad.json()["error"], "invalid_format");
    assert_eq!(
        app.get("/api/admin/projects/999/export")
            .send()
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

// ---------------------------------------------------------------------------
// Feedback listing, filters and triage

async fn seeded() -> (App, i64, i64) {
    let app = App::admin().await;
    let a = app.create_project("Alpha").await;
    let b = app.create_project("Beta").await;
    let (ka, kb) = (key_of(&a), key_of(&b));
    app.submit_ok(
        &ka,
        "10.0.0.1",
        "bug",
        "Inventory crash",
        "The inventory screen crashes the game when opened",
    )
    .await;
    app.submit_ok(
        &ka,
        "10.0.0.2",
        "bug",
        "Freeze on loading",
        "Game freezes on the loading screen and gets stuck",
    )
    .await;
    app.submit_ok(
        &ka,
        "10.0.0.3",
        "feedback",
        "Soundtrack",
        "I love the soundtrack and the art style",
    )
    .await;
    app.submit_ok(
        &kb,
        "10.0.0.4",
        "feedback",
        "Dark mode",
        "Please add a dark mode option to the settings",
    )
    .await;
    app.submit_ok(
        &kb,
        "10.0.0.5",
        "bug",
        "Login broken",
        "Cannot log in with my email address",
    )
    .await;
    app.embed_all().await;
    (app, a["id"].as_i64().unwrap(), b["id"].as_i64().unwrap())
}

#[tokio::test]
async fn list_filters_and_pagination() {
    let (app, a, b) = seeded().await;
    let all = app.feedback("").await;
    assert_eq!(all["total"], 5);
    assert_eq!(all["semantic"], false);
    // Newest first.
    assert_eq!(titles(&all)[0], "Login broken");

    assert_eq!(app.feedback(&format!("project_id={a}")).await["total"], 3);
    assert_eq!(
        app.feedback(&format!("project_id={b}&kind=bug")).await["total"],
        1
    );
    assert_eq!(app.feedback("kind=feedback").await["total"], 2);

    let page1 = app.feedback("limit=2").await;
    let page2 = app.feedback("limit=2&offset=2").await;
    assert_eq!(page1["items"].as_array().unwrap().len(), 2);
    assert_eq!(page1["total"], 5);
    assert!(ids(&page1).iter().all(|id| !ids(&page2).contains(id)));

    let now = feedback_harvest::db::now();
    assert_eq!(
        app.feedback(&format!("from={}", now + 100)).await["total"],
        0
    );
    assert_eq!(app.feedback(&format!("to={}", now + 100)).await["total"], 5);
    assert_eq!(
        app.feedback(&format!("from={}&to={}", now - 100, now + 100))
            .await["total"],
        5
    );

    for bad in [
        "kind=x",
        "status=x",
        "limit=abc",
        "mode=magic",
        "project_id=1.5",
    ] {
        let r = app.get(&format!("/api/admin/feedback?{bad}")).send().await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{bad}");
        assert_eq!(r.json()["error"], "invalid_query");
    }
}

#[tokio::test]
async fn status_filters_and_triage() {
    let (app, _, _) = seeded().await;
    let id = ids(&app.feedback("").await)[0];

    let r = app
        .patch(&format!("/api/admin/feedback/{id}"))
        .json(json!({ "status": "resolved", "admin_note": "fixed in 1.3", "kind": "feedback" }))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = r.json();
    assert_eq!(r["status"], "resolved");
    assert_eq!(r["admin_note"], "fixed in 1.3");
    assert_eq!(r["kind"], "feedback");

    assert_eq!(app.feedback("").await["total"], 4, "open excludes resolved");
    assert_eq!(app.feedback("status=resolved").await["total"], 1);
    assert_eq!(app.feedback("status=all").await["total"], 5);

    let other = ids(&app.feedback("").await)[0];
    app.patch(&format!("/api/admin/feedback/{other}"))
        .json(json!({ "status": "spam" }))
        .send()
        .await;
    assert_eq!(
        app.feedback("status=all").await["total"],
        4,
        "all excludes spam"
    );
    assert_eq!(app.feedback("status=any").await["total"], 5);
    let restored = app
        .patch(&format!("/api/admin/feedback/{other}"))
        .json(json!({ "status": "triaged" }))
        .send()
        .await
        .json();
    assert_eq!(restored["spam_reason"], json!(null));
    assert_eq!(app.feedback("status=triaged").await["total"], 1);

    for (body, code) in [
        (json!({ "status": "done" }), "invalid_status"),
        (json!({ "kind": "rant" }), "invalid_kind"),
        (
            json!({ "admin_note": "n".repeat(10_001) }),
            "invalid_admin_note",
        ),
    ] {
        let r = app
            .patch(&format!("/api/admin/feedback/{id}"))
            .json(body)
            .send()
            .await;
        assert_eq!(r.json()["error"], code);
    }
    assert_eq!(
        app.patch("/api/admin/feedback/999")
            .json(json!({ "status": "new" }))
            .send()
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn detail_includes_similar_items() {
    let (app, a, _) = seeded().await;
    let list = app
        .feedback(&format!("project_id={a}&q=inventory&mode=keyword"))
        .await;
    let id = ids(&list)[0];
    let r = app.get(&format!("/api/admin/feedback/{id}")).send().await;
    assert_eq!(r.status, StatusCode::OK);
    let r = r.json();
    assert_eq!(r["item"]["id"], id);
    let similar = r["similar"].as_array().unwrap();
    assert!(!similar.is_empty() && similar.len() <= 5);
    assert!(
        similar
            .iter()
            .all(|s| s["project_id"] == a && s["id"] != id)
    );
    assert!(similar.iter().all(|s| s["score"].is_number()));
    assert_eq!(
        similar[0]["title"], "Freeze on loading",
        "crash and freeze are related: {similar:?}"
    );
    assert_eq!(
        app.get("/api/admin/feedback/999").send().await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn delete_and_bulk_actions() {
    let (app, _, _) = seeded().await;
    let all = ids(&app.feedback("").await);

    assert_eq!(
        app.delete(&format!("/api/admin/feedback/{}", all[0]))
            .send()
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.delete(&format!("/api/admin/feedback/{}", all[0]))
            .send()
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(app.state.index.get(all[0]), None);

    let r = app
        .post("/api/admin/feedback/bulk")
        .json(
            json!({ "ids": [all[1], all[2], 9999], "action": "set_status", "status": "archived" }),
        )
        .send()
        .await;
    assert_eq!(r.json()["affected"], 2);
    assert_eq!(app.feedback("status=archived").await["total"], 2);

    let r = app
        .post("/api/admin/feedback/bulk")
        .json(json!({ "ids": [all[3], all[4]], "action": "delete" }))
        .send()
        .await;
    assert_eq!(r.json()["affected"], 2);
    assert_eq!(app.feedback("status=any").await["total"], 2);

    for (body, code) in [
        (json!({ "ids": [], "action": "delete" }), "invalid_ids"),
        (
            json!({ "ids": (0..501).collect::<Vec<_>>(), "action": "delete" }),
            "invalid_ids",
        ),
        (json!({ "ids": [1], "action": "explode" }), "invalid_action"),
        (
            json!({ "ids": [1], "action": "set_status", "status": "nope" }),
            "invalid_status",
        ),
    ] {
        let r = app.post("/api/admin/feedback/bulk").json(body).send().await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
        assert_eq!(r.json()["error"], code);
    }
}

// ---------------------------------------------------------------------------
// Search

#[tokio::test]
async fn keyword_search_ranks_matches() {
    let (app, _, _) = seeded().await;
    let r = app.feedback("q=inventory&mode=keyword").await;
    assert_eq!(r["mode"], "keyword");
    assert_eq!(titles(&r), vec!["Inventory crash"]);
    assert!(r["items"][0]["score"].as_f64().unwrap() > 0.0);
    // Stemming: "crashing" matches "crashes"; prefix: "invent" matches "inventory".
    assert_eq!(
        titles(&app.feedback("q=crashing&mode=keyword").await),
        vec!["Inventory crash"]
    );
    assert_eq!(
        titles(&app.feedback("q=invent&mode=keyword").await),
        vec!["Inventory crash"]
    );
}

#[tokio::test]
async fn fuzzy_search_tolerates_typos() {
    let (app, _, _) = seeded().await;
    for q in ["inventroy", "invetory", "soundtrak", "lodaing"] {
        let r = app.feedback(&format!("q={q}&mode=fuzzy")).await;
        assert_eq!(r["total"], 1, "{q}: {r}");
    }
    assert_eq!(
        titles(&app.feedback("q=soundtrak&mode=fuzzy").await),
        vec!["Soundtrack"]
    );
    assert_eq!(app.feedback("q=zebra&mode=fuzzy").await["total"], 0);
}

#[tokio::test]
async fn semantic_search_finds_related_vocabulary() {
    let (app, a, _) = seeded().await;
    let r = app
        .feedback(&format!("q=hang&mode=semantic&project_id={a}"))
        .await;
    assert_eq!(r["mode"], "semantic");
    assert_eq!(r["semantic"], true);
    let t = titles(&r);
    assert!(!t.is_empty());
    assert!(
        t[0] == "Freeze on loading" || t[0] == "Inventory crash",
        "hang should surface crash/freeze reports first: {t:?}"
    );
    assert!(!t.contains(&"Soundtrack".to_string()), "{t:?}");
}

#[tokio::test]
async fn hybrid_search_combines_signals() {
    let (app, _, _) = seeded().await;
    let r = app.feedback("q=inventroy%20crash").await;
    assert_eq!(r["mode"], "hybrid");
    assert_eq!(r["semantic"], true);
    assert_eq!(titles(&r)[0], "Inventory crash");
    // Filters still apply to search results.
    assert_eq!(app.feedback("q=inventory&kind=feedback").await["total"], 0);
    // Pagination over search results.
    let p = app.feedback("q=game&limit=1").await;
    assert_eq!(p["items"].as_array().unwrap().len(), 1);
    assert!(p["total"].as_u64().unwrap() >= 2);
}

#[tokio::test]
async fn semantic_falls_back_when_embeddings_disabled() {
    let mut app = App::with(|c| c.embed = feedback_harvest::config::EmbedConfig::Disabled).await;
    app.login().await;
    let key = key_of(&app.create_project("A").await);
    app.submit_ok(
        &key,
        "2.2.2.2",
        "bug",
        "Inventory crash",
        "inventory crashes",
    )
    .await;
    let r = app.feedback("q=inventory&mode=semantic").await;
    assert_eq!(r["semantic"], false);
    assert_eq!(r["mode"], "hybrid");
    assert_eq!(r["total"], 1);
    let stats = app.get("/api/admin/stats").send().await.json();
    assert_eq!(stats["embeddings"]["enabled"], false);
}

#[tokio::test]
async fn semantic_falls_back_when_provider_fails() {
    let mut app = App::with(|c| {
        c.embed = feedback_harvest::config::EmbedConfig::OpenAi {
            url: "http://127.0.0.1:9".into(),
            model: "m".into(),
            api_key: None,
        }
    })
    .await;
    app.login().await;
    let key = key_of(&app.create_project("A").await);
    app.submit_ok(
        &key,
        "2.2.2.2",
        "bug",
        "Inventory crash",
        "inventory crashes",
    )
    .await;
    let r = app.feedback("q=inventory").await;
    assert_eq!(r["semantic"], false);
    assert_eq!(r["total"], 1);
}

// ---------------------------------------------------------------------------
// Stats and maintenance

#[tokio::test]
async fn stats_shape() {
    let (app, a, _) = seeded().await;
    let s = app.get("/api/admin/stats").send().await.json();
    assert_eq!(s["totals"]["total"], 5);
    assert_eq!(s["totals"]["bugs"], 3);
    assert_eq!(s["totals"]["feedback"], 2);
    assert_eq!(s["totals"]["last_7d"], 5);
    let daily = s["daily"].as_array().unwrap();
    assert_eq!(daily.len(), 30);
    let today = &daily[29];
    assert_eq!(today["bug"], 3);
    assert_eq!(today["feedback"], 2);
    assert!(daily[0]["day"].as_str().unwrap() < today["day"].as_str().unwrap());
    for reason in [
        "rate_limited",
        "honeypot",
        "pow_insufficient",
        "duplicate",
        "spam_score",
        "too_fast",
        "near_duplicate",
    ] {
        assert_eq!(s["blocked"][reason], 0, "{reason}");
    }
    assert_eq!(s["embeddings"]["enabled"], true);
    assert_eq!(s["embeddings"]["model"], "hash-v1-384");
    assert_eq!(s["embeddings"]["embedded"], 5);
    assert_eq!(s["embeddings"]["pending"], 0);

    let scoped = app
        .get(&format!("/api/admin/stats?project_id={a}"))
        .send()
        .await
        .json();
    assert_eq!(scoped["totals"]["total"], 3);
}

#[tokio::test]
async fn reindex_rebuilds_vectors() {
    let (app, _, _) = seeded().await;
    let r = app.post("/api/admin/reindex").send().await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    assert_eq!(app.state.index.len(), 0);
    assert_eq!(
        app.get("/api/admin/stats").send().await.json()["embeddings"]["pending"],
        5
    );
    app.embed_all().await;
    assert_eq!(app.state.index.len(), 5);
}
