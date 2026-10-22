mod common;

use axum::http::StatusCode;
use common::App;
use serde_json::json;

/// The admin build is optional for Rust-only runs (rust-embed `allow_missing`).
fn admin_built() -> bool {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/admin/dist/favicon.svg"
    ))
    .exists()
}

#[tokio::test]
async fn root_icons_are_served_from_the_admin_build() {
    let app = App::new().await;
    for (path, mime) in [
        ("/favicon.svg", "image/svg+xml"),
        ("/favicon.ico", "image/"),
        ("/apple-touch-icon.png", "image/png"),
    ] {
        let r = app.get(path).anonymous().send().await;
        if !admin_built() {
            assert_eq!(r.status, StatusCode::NOT_FOUND, "{path}");
            continue;
        }
        assert_eq!(r.status, StatusCode::OK, "{path}");
        assert!(
            r.header("content-type").unwrap().starts_with(mime),
            "{path}"
        );
        assert_eq!(
            r.header("cache-control"),
            Some("public, max-age=86400"),
            "{path}"
        );
        assert_eq!(
            r.header("x-content-type-options"),
            Some("nosniff"),
            "{path}"
        );
    }
}

#[tokio::test]
async fn admin_index_carries_brand_meta() {
    if !admin_built() {
        return;
    }
    let app = App::new().await;
    let r = app.get("/admin/").anonymous().send().await;
    assert_eq!(r.status, StatusCode::OK);
    for needle in [
        "<title>Feedback Harvest</title>",
        "name=\"description\"",
        "rel=\"manifest\"",
        "noindex, nofollow",
        "og:image",
    ] {
        assert!(r.text.contains(needle), "missing {needle}");
    }
    let manifest = app.get("/admin/site.webmanifest").anonymous().send().await;
    assert_eq!(manifest.status, StatusCode::OK);
    assert!(manifest.text.contains("\"name\": \"Feedback Harvest\""));
}

#[tokio::test]
async fn hosted_page_has_title_description_and_social_meta() {
    let app = App::admin().await;
    app.post("/api/admin/projects")
        .json(json!({ "title": "Star \"Quest\" <II>", "slug": "sq", "description": "A space\n  game & more" }))
        .send()
        .await;
    let r = app.get("/s/sq").anonymous().send().await;
    assert_eq!(r.status, StatusCode::OK);
    let t = &r.text;
    assert!(
        t.contains(
            "<title>Send feedback: Star &quot;Quest&quot; &lt;II&gt; | Feedback Harvest</title>"
        ),
        "{t}"
    );
    // Whitespace in the description is collapsed and escaped.
    assert!(
        t.contains("<meta name=\"description\" content=\"A space game &amp; more\">"),
        "{t}"
    );
    assert!(t.contains(
        "<meta property=\"og:title\" content=\"Send feedback: Star &quot;Quest&quot; &lt;II&gt;\">"
    ));
    assert!(t.contains("<meta name=\"robots\" content=\"noindex, nofollow\">"));
    assert!(t.contains("rel=\"icon\" href=\"/favicon.svg\""));
    assert!(t.contains("name=\"theme-color\""));
    assert!(t.contains("Powered by Feedback Harvest"));

    // Without a description a sensible default is used.
    app.post("/api/admin/projects")
        .json(json!({ "title": "Plain", "slug": "plain" }))
        .send()
        .await;
    let r = app.get("/s/plain").anonymous().send().await;
    assert!(
        r.text
            .contains("content=\"Report a bug or share feedback about Plain.\"")
    );

    let missing = app.get("/s/missing").anonymous().send().await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert!(
        missing
            .text
            .contains("<title>Not found | Feedback Harvest</title>")
    );
}
