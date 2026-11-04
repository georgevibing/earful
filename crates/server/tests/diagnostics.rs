mod common;
use axum::http::StatusCode;
use common::{App, key_of};
use serde_json::json;

#[tokio::test]
async fn diagnostics_roundtrip_filters_exports_and_validation() {
    let app = App::admin().await;
    let p = app.create_project("Crash game").await;
    let key = key_of(&p);
    let diagnostics = json!({"stack_trace":"panic: inventory\n  at save.rs:42", "logs":"x".repeat(40000),"build_id":"abc123","context":{"scene":"inventory"}});
    for (i, kind) in ["crash", "exception", "log"].iter().enumerate() {
        let r = app.submit(&key, &format!("2.2.2.{}", i+1), json!({"kind":kind,"body":"Inventory save failed unexpectedly","diagnostics":diagnostics})).await;
        assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.text);
        let page = app.feedback(&format!("kind={kind}&status=any")).await;
        assert_eq!(page["total"], 1);
        assert_eq!(page["items"][0]["diagnostics"], diagnostics);
        let id = page["items"][0]["id"].as_i64().unwrap();
        assert_eq!(
            app.get(&format!("/api/admin/feedback/{id}"))
                .send()
                .await
                .json()["item"]["diagnostics"],
            diagnostics
        );
        assert_eq!(
            app.feedback(&format!("kind={kind}&q=Inventory&mode=keyword&status=any"))
                .await["total"],
            1
        );
    }
    for bad in [json!([]), json!({"logs":"a".repeat(65536)})] {
        let r = app
            .submit(
                &key,
                "3.3.3.3",
                json!({"kind":"crash","body":"Failed saving state","diagnostics":bad}),
            )
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
        assert_eq!(r.json()["error"], "invalid_diagnostics");
    }
    app.embed_all().await;
    assert_eq!(app.feedback("status=open").await["total"], 3);
    let id = p["id"].as_i64().unwrap();
    let export = app
        .get(&format!("/api/admin/projects/{id}/export?format=csv"))
        .send()
        .await;
    assert!(export.text.contains("diagnostics"));
    assert!(export.text.contains("abc123"));
    let stats = app.get("/api/admin/stats").send().await.json();
    assert_eq!(stats["kind_counts"]["crash"], 1);
}

#[tokio::test]
async fn distinct_diagnostics_are_not_discarded_as_duplicates() {
    let app = App::admin().await;
    let key = key_of(&app.create_project("Distinct stacks").await);
    for stack in ["save.rs:1", "save.rs:2", "save.rs:2"] {
        let r=app.submit(&key,"4.4.4.4",json!({"kind":"crash","body":"Game crashed while saving inventory","diagnostics":{"stack_trace":stack}})).await;
        assert_eq!(r.status, StatusCode::ACCEPTED);
    }
    app.embed_all().await;
    assert_eq!(app.feedback("kind=crash&status=open").await["total"], 2);
}
