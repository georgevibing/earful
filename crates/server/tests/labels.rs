//! Labels use real HTTP routes and a migrated, isolated SQLite database.
mod common;
use axum::http::StatusCode;
use common::{App, ids, key_of};
use serde_json::{Value, json};

async fn label(app: &App, project: &Value, name: &str) -> Value {
    let r = app.post("/api/admin/labels").json(json!({"project_id":project["id"],"name":name,"description":"Reports about save files","color":"blue"})).send().await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
    r.json()
}
async fn seed(app: &App) -> (Value, Vec<i64>) {
    let p = app.create_project("Starfall").await;
    for (ip, title, body) in [
        (
            "1.1.1.1",
            "Save crash",
            "The game crashes when saving the inventory",
        ),
        (
            "1.1.1.2",
            "Loading crash",
            "The game crashes on loading an inventory save",
        ),
        (
            "1.1.1.3",
            "Small fonts",
            "Please make the text larger in menus",
        ),
    ] {
        app.submit_ok(&key_of(&p), ip, "bug", title, body).await;
    }
    let rows = app.feedback("status=any").await;
    let mut items = ids(&rows);
    items.sort();
    (p, items)
}
async fn assign(app: &App, ids: &[i64], labels: &[i64], action: &str) -> common::Resp {
    app.post("/api/admin/feedback/labels")
        .json(json!({"ids":ids,"label_ids":labels,"action":action}))
        .send()
        .await
}

#[tokio::test]
async fn labels_crud_validation_and_project_isolation() {
    let app = App::admin().await;
    let p = app.create_project("Game").await;
    let other = app.create_project("Other").await;
    let l = label(&app, &p, " Save corruption ").await;
    assert_eq!(l["name"], "Save corruption");
    assert_eq!(l["feedback_count"], 0);
    assert_eq!(l["open_count"], 0);
    let lid = l["id"].as_i64().unwrap();
    let duplicate = app
        .post("/api/admin/labels")
        .json(json!({"project_id":p["id"],"name":"SAVE CORRUPTION"}))
        .send()
        .await;
    assert_eq!(duplicate.status, StatusCode::CONFLICT);
    label(&app, &other, "Save corruption").await;
    let list = app
        .get(&format!("/api/admin/labels?project_id={}", p["id"]))
        .send()
        .await
        .json();
    assert_eq!(list.as_array().unwrap().len(), 1);
    for body in [
        json!({"name":""}),
        json!({"name":"x".repeat(65)}),
        json!({"name":"hello\nthere"}),
        json!({"color":"url(evil)"}),
        json!({"description":"x".repeat(501)}),
        json!({"project_id":other["id"]}),
        json!({}),
    ] {
        assert_eq!(
            app.patch(&format!("/api/admin/labels/{lid}"))
                .json(body)
                .send()
                .await
                .status,
            StatusCode::BAD_REQUEST
        );
    }
    let renamed = app
        .patch(&format!("/api/admin/labels/{lid}"))
        .json(json!({"name":"Save integrity","color":"red","description":"Broken save files"}))
        .send()
        .await;
    assert_eq!(renamed.status, StatusCode::OK);
    assert_eq!(renamed.json()["id"], lid);
    assert_eq!(renamed.json()["name"], "Save integrity");
    assert_eq!(
        app.delete(&format!("/api/admin/labels/{lid}"))
            .send()
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.delete(&format!("/api/admin/labels/{lid}"))
            .send()
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.get("/api/admin/labels?project_id=999999")
            .send()
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.get("/api/admin/labels?project_id=bad")
            .send()
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        app.post("/api/admin/labels")
            .json(json!({"project_id":999999,"name":"x"}))
            .send()
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn labels_assignment_is_atomic_idempotent_and_preserves_other_labels() {
    let app = App::admin().await;
    let (p, ids) = seed(&app).await;
    let a = label(&app, &p, "Save").await["id"].as_i64().unwrap();
    let b = label(&app, &p, "Crash").await["id"].as_i64().unwrap();
    assert_eq!(
        assign(&app, &ids[..2], &[a], "add").await.json()["affected"],
        2
    );
    assert_eq!(
        assign(&app, &ids[..2], &[a, a], "add").await.json()["affected"],
        0
    );
    assert_eq!(
        assign(&app, &[ids[0], ids[0]], &[b], "add").await.json()["affected"],
        1
    );
    let row = app
        .get(&format!("/api/admin/feedback/{}", ids[0]))
        .send()
        .await
        .json();
    assert_eq!(row["item"]["labels"].as_array().unwrap().len(), 2);
    assert_eq!(row["item"]["status"], "new");
    // A valid first item followed by an invalid id must roll back the first write.
    let r = assign(&app, &[ids[2], 999999], &[a], "add").await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(app.feedback("unlabeled=true&status=any").await["total"], 1);
    let other = app.create_project("Other").await;
    let foreign = label(&app, &other, "Save").await["id"].as_i64().unwrap();
    assert_eq!(
        assign(&app, &ids, &[foreign], "add").await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        assign(&app, &[ids[0]], &[b], "remove").await.json()["affected"],
        1
    );
    assert_eq!(app.feedback(&format!("label_ids={a}")).await["total"], 2);
    assert_eq!(
        assign(&app, &[ids[0]], &[b], "set").await.json()["affected"],
        1
    );
    assert_eq!(
        assign(&app, &[ids[0]], &[], "set").await.json()["affected"],
        1
    );
    assert_eq!(
        assign(&app, &ids, &[], "add").await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        assign(&app, &[], &[a], "add").await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        assign(&app, &ids, &[999999], "add").await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn labels_filters_apply_to_search_modes_counts_and_pagination() {
    let app = App::admin().await;
    let (p, ids) = seed(&app).await;
    let a = label(&app, &p, "Save").await["id"].as_i64().unwrap();
    let b = label(&app, &p, "Crash").await["id"].as_i64().unwrap();
    assign(&app, &ids[..2], &[a], "add").await;
    assign(&app, &[ids[0]], &[b], "add").await;
    app.embed_all().await;
    assert_eq!(
        app.feedback(&format!("label_ids={a},{b}")).await["total"],
        1
    );
    assert_eq!(
        app.feedback(&format!("label_ids={a},{b}&label_match=any"))
            .await["total"],
        2
    );
    let page = app
        .feedback(&format!("label_ids={a}&limit=1&offset=1"))
        .await;
    assert_eq!(page["total"], 2);
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert!(
        page["items"][0]["labels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["id"] == a)
    );
    assert_eq!(app.feedback("unlabeled=true").await["total"], 1);
    assert_eq!(app.feedback("label_ids=999999").await["total"], 0);
    for mode in ["keyword", "fuzzy", "semantic", "hybrid"] {
        let result = app
            .feedback(&format!("label_ids={b}&q=crash&mode={mode}"))
            .await;
        assert_eq!(result["total"], 1, "{mode}: {result}");
        assert_eq!(result["items"][0]["id"], ids[0]);
    }
    for q in [
        "label_ids=-1",
        "label_ids=bad",
        "label_ids=1,,2",
        "label_match=nope",
        "unlabeled=perhaps",
        "unlabeled=true&label_ids=1",
    ] {
        assert_eq!(
            app.get(&format!("/api/admin/feedback?{q}"))
                .send()
                .await
                .status,
            StatusCode::BAD_REQUEST,
            "{q}"
        );
    }
    app.patch(&format!("/api/admin/feedback/{}", ids[1]))
        .json(json!({"status":"spam"}))
        .send()
        .await;
    assert_eq!(
        app.feedback(&format!("label_ids={a}&status=all")).await["total"],
        1
    );
    assert_eq!(
        app.feedback(&format!("label_ids={a}&status=any")).await["total"],
        2
    );
    let labels = app.get("/api/admin/labels").send().await.json();
    assert_eq!(labels[0]["feedback_count"], 1);
    assert_eq!(labels[0]["open_count"], 1);
}

#[tokio::test]
async fn labels_are_in_exports_and_cascade_without_deleting_feedback() {
    let app = App::admin().await;
    let (p, ids) = seed(&app).await;
    let l = label(&app, &p, "Save, crash").await;
    let lid = l["id"].as_i64().unwrap();
    assign(&app, &[ids[0]], &[lid], "add").await;
    let data = app
        .get(&format!(
            "/api/admin/projects/{}/export?format=json",
            p["id"]
        ))
        .send()
        .await
        .json();
    assert_eq!(data[0]["labels"][0]["name"], "Save, crash");
    let csv = app
        .get(&format!(
            "/api/admin/projects/{}/export?format=csv",
            p["id"]
        ))
        .send()
        .await;
    assert!(csv.text.contains("created_at_iso,labels\r\n"));
    assert!(csv.text.contains("Save, crash"));
    app.patch(&format!("/api/admin/labels/{lid}"))
        .json(json!({"name":"Save integrity"}))
        .send()
        .await;
    assert_eq!(
        app.get(&format!("/api/admin/feedback/{}", ids[0]))
            .send()
            .await
            .json()["item"]["labels"][0]["name"],
        "Save integrity"
    );
    app.delete(&format!("/api/admin/labels/{lid}")).send().await;
    assert_eq!(app.feedback("status=any").await["total"], 3);
    assert_eq!(app.sql_i64("SELECT COUNT(*) FROM feedback_labels").await, 0);
    let replacement = label(&app, &p, "Other").await;
    assert_ne!(
        replacement["id"], lid,
        "deleted label ids must never be reused"
    );
    assign(
        &app,
        &[ids[0]],
        &[replacement["id"].as_i64().unwrap()],
        "add",
    )
    .await;
    app.delete(&format!("/api/admin/feedback/{}", ids[0]))
        .send()
        .await;
    assert_eq!(app.sql_i64("SELECT COUNT(*) FROM feedback_labels").await, 0);
    app.delete(&format!("/api/admin/projects/{}", p["id"]))
        .send()
        .await;
    assert_eq!(app.sql_i64("SELECT COUNT(*) FROM labels").await, 0);
}

#[tokio::test]
async fn labels_limits_and_batch_rollback() {
    let app = App::admin().await;
    let (p, ids) = seed(&app).await;
    let mut labels = vec![];
    for i in 0..21 {
        labels.push(
            label(&app, &p, &format!("Theme {i}")).await["id"]
                .as_i64()
                .unwrap(),
        );
    }
    assert_eq!(
        assign(&app, &[ids[0]], &labels[..20], "set").await.status,
        StatusCode::OK
    );
    assert_eq!(
        assign(&app, &[ids[1], ids[0]], &labels[20..], "add")
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(app.feedback("unlabeled=true").await["total"], 2);
    assert_eq!(
        assign(&app, &ids, &labels, "set").await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        assign(&app, &vec![ids[0]; 501], &labels[..1], "add")
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    for i in 21..200 {
        label(&app, &p, &format!("Theme {i}")).await;
    }
    assert_eq!(
        app.post("/api/admin/labels")
            .json(json!({"project_id":p["id"],"name":"Over limit"}))
            .send()
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    // Editing remains possible at the project limit.
    assert_eq!(
        app.patch(&format!("/api/admin/labels/{}", labels[0]))
            .json(json!({"name":"Renamed at limit"}))
            .send()
            .await
            .status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn labels_require_session_and_reject_cross_origin_writes() {
    let app = App::admin().await;
    for response in [
        app.get("/api/admin/labels").anonymous().send().await,
        app.post("/api/admin/labels")
            .anonymous()
            .json(json!({"project_id":1,"name":"x"}))
            .send()
            .await,
        app.patch("/api/admin/labels/1")
            .anonymous()
            .json(json!({"name":"x"}))
            .send()
            .await,
        app.delete("/api/admin/labels/1").anonymous().send().await,
        app.post("/api/admin/feedback/labels")
            .anonymous()
            .json(json!({"ids":[1],"label_ids":[],"action":"set"}))
            .send()
            .await,
    ] {
        assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    }
    let r = app
        .post("/api/admin/labels")
        .header("origin", "https://evil.example")
        .json(json!({"project_id":1,"name":"x"}))
        .send()
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
}

#[test]
fn labels_migration_preserves_existing_data_and_persists() {
    use feedback_harvest::db;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("upgrade.db");
    // Build the exact previous schema from the append-only migration source.
    let source = include_str!("../src/db.rs");
    let old: Vec<_> = source
        .split("r#\"")
        .skip(1)
        .take(3)
        .map(|s| s.split("\"#").next().unwrap())
        .collect();
    {
        let mut c = db::open(&file).unwrap();
        for sql in old {
            c.execute_batch(sql).unwrap();
        }
        c.pragma_update(None, "user_version", 3).unwrap();
        c.execute("INSERT INTO projects(id,slug,title,ingest_key,created_at,updated_at) VALUES(1,'old','Existing','key',1,1)",[]).unwrap();
        db::migrate(&mut c).unwrap();
        c.execute("INSERT INTO labels(project_id,name,name_key,created_at,updated_at) VALUES(1,'Save','save',1,1)",[]).unwrap();
    }
    let mut c = db::open(&file).unwrap();
    db::migrate(&mut c).unwrap();
    assert_eq!(
        c.query_row("SELECT title FROM projects", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "Existing"
    );
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM labels", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(
        c.execute(
            "INSERT INTO feedback_labels(feedback_id,label_id,project_id) VALUES(999,1,1)",
            []
        )
        .is_err()
    );
}
