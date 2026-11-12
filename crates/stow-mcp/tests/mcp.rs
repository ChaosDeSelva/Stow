//! In-process MCP tests: an rmcp client talks to `StowMcp` over an in-memory duplex pipe.

use std::path::Path;

use rmcp::model::{
    CallToolRequestParams, CallToolResult, ReadResourceRequestParams, ResourceContents,
};
use rmcp::service::{RoleClient, RunningService};
use rmcp::ServiceExt;
use serde_json::{json, Value};
use stow_core::models::{JobStatus, ParentType, TargetType};
use stow_core::repo::{buckets, items, reminders, search, tasks};
use stow_core::Stow;
use stow_mcp::StowMcp;

type Client = RunningService<RoleClient, ()>;

/// Every tool the server offers, sorted. Mutating tools are the ones hidden in read-only mode.
const READ_TOOLS: &[&str] = &["doctor", "get_item", "list_buckets", "list_items", "search"];
const WRITE_TOOLS: &[&str] = &[
    "add_links",
    "add_reminder",
    "add_task",
    "archive_item",
    "complete_task",
    "create_bucket",
    "delete_item",
    "export",
    "move_item",
    "update_notes",
];

async fn connect(dir: &Path, read_only: bool) -> Client {
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let stow = Stow::open(dir).unwrap();
    tokio::spawn(async move {
        let running = StowMcp::new(stow, read_only)
            .serve(server_io)
            .await
            .expect("server handshake");
        let _ = running.waiting().await;
    });
    ().serve(client_io).await.expect("client handshake")
}

async fn try_call(
    client: &Client,
    name: &str,
    args: Value,
) -> Result<CallToolResult, rmcp::ServiceError> {
    let Value::Object(args) = args else {
        panic!("tool arguments must be an object")
    };
    client
        .call_tool(CallToolRequestParams::new(name.to_string()).with_arguments(args))
        .await
}

/// Call a tool and return its structured result; panics on a tool-level error.
async fn call(client: &Client, name: &str, args: Value) -> Value {
    let r = try_call(client, name, args).await.unwrap();
    assert_ne!(r.is_error, Some(true), "{name} failed: {:?}", r.content);
    r.structured_content.expect("structured content")
}

/// Call a tool expecting a tool-level error; returns the error text.
async fn call_err(client: &Client, name: &str, args: Value) -> String {
    let r = try_call(client, name, args).await.unwrap();
    assert_eq!(
        r.is_error,
        Some(true),
        "{name} should fail: {:?}",
        r.content
    );
    r.content[0].as_text().unwrap().text.clone()
}

fn seed_item(db: &stow_core::Db, url: &str, title: &str, bucket: Option<&str>) -> String {
    items::create(
        db,
        items::NewItem {
            url: url.into(),
            bucket_id: bucket.map(str::to_string),
            title: Some(title.into()),
        },
    )
    .unwrap()
    .value
    .id
}

fn write_snapshot(db: &stow_core::Db, id: &str, title: &str, text: &str) {
    std::fs::write(
        db.data_dir().join("snapshots").join(format!("{id}.md")),
        text,
    )
    .unwrap();
    let bucket = items::get(db, id).unwrap().bucket_id;
    search::index(db, "snapshot", id, Some(id), bucket.as_deref(), title, text).unwrap();
    items::set_snapshot_done(db, id).unwrap();
}

#[tokio::test]
async fn tools_list_matches_contract() {
    let dir = tempfile::tempdir().unwrap();
    let client = connect(dir.path(), false).await;
    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    let mut expected: Vec<&str> = READ_TOOLS.iter().chain(WRITE_TOOLS).copied().collect();
    expected.sort_unstable();
    assert_eq!(names, expected);
    for t in &tools {
        assert!(
            t.description.as_deref().is_some_and(|d| !d.is_empty()),
            "{} has no description",
            t.name
        );
        let read_only = t
            .annotations
            .as_ref()
            .and_then(|a| a.read_only_hint)
            .unwrap_or(false);
        assert_eq!(
            read_only,
            READ_TOOLS.contains(&t.name.as_ref()),
            "{} read_only_hint",
            t.name
        );
    }
    let search = tools.iter().find(|t| t.name == "search").unwrap();
    let props = &search.input_schema["properties"];
    assert_eq!(props["query"]["type"], "string");
    assert!(props.get("limit").is_some(), "{props}");
    assert_eq!(search.input_schema["required"], json!(["query"]));
    let info = client.peer_info().unwrap();
    assert_eq!(info.server_info.as_ref().unwrap().name, "stow");
    assert!(info.capabilities.resources.is_some());
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn search_and_get_item_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let db = Stow::open(dir.path()).unwrap().db().clone();
    let b = buckets::create(&db, "Reading", Some("📚")).unwrap();
    let id = seed_item(
        &db,
        "https://example.com/okapi",
        "Okapi field guide",
        Some(&b.id),
    );
    items::set_notes(&db, &id, "Compare with the **giraffe** chapter.").unwrap();
    tasks::create(&db, ParentType::Item, &id, "Read chapter 3").unwrap();
    reminders::create(
        &db,
        TargetType::Item,
        &id,
        "2030-01-01T09:00:00Z",
        Some("library"),
    )
    .unwrap();
    write_snapshot(
        &db,
        &id,
        "Okapi field guide",
        "# Okapi\n\nThe okapi is a forest giraffe of the Congo basin.",
    );
    let long_id = seed_item(&db, "https://example.com/long", "Long read", None);
    write_snapshot(&db, &long_id, "Long read", &"lorem ipsum ".repeat(10_000));

    let client = connect(dir.path(), false).await;

    let hits = call(&client, "search", json!({"query": "forest giraffe"})).await;
    let hits = hits["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0]["doc_type"], "snapshot");
    assert_eq!(hits[0]["item_id"], id);
    assert_eq!(hits[0]["bucket_id"], b.id);
    assert_eq!(hits[0]["title"], "Okapi field guide");
    assert!(hits[0]["snippet"].as_str().unwrap().contains("[forest]"));
    let limited = call(&client, "search", json!({"query": "example", "limit": 1})).await;
    assert_eq!(limited["hits"].as_array().unwrap().len(), 1);

    let item = call(&client, "get_item", json!({"id": id})).await;
    assert_eq!(item["id"], id);
    assert_eq!(item["title"], "Okapi field guide");
    assert_eq!(item["url"], "https://example.com/okapi");
    assert_eq!(item["kind"], "article");
    assert_eq!(item["bucket"]["id"], b.id);
    assert_eq!(item["bucket"]["name"], "Reading");
    assert_eq!(item["archived"], false);
    assert_eq!(item["notes_md"], "Compare with the **giraffe** chapter.");
    assert_eq!(item["tasks"][0]["title"], "Read chapter 3");
    assert_eq!(item["tasks"][0]["done"], false);
    assert_eq!(item["reminders"][0]["message"], "library");
    assert_eq!(item["reminders"][0]["at"], "2030-01-01T09:00:00.000000Z");
    assert!(item["text"]
        .as_str()
        .unwrap()
        .contains("forest giraffe of the Congo basin"));
    assert_eq!(item["text_truncated"], false);
    assert_eq!(item["transcripts"], json!([]));
    assert_eq!(item["ai_runs"], json!([]));

    let no_text = call(
        &client,
        "get_item",
        json!({"id": id, "include_text": false}),
    )
    .await;
    assert!(no_text["text"].is_null());

    let long = call(&client, "get_item", json!({"id": long_id})).await;
    assert_eq!(long["text_truncated"], true);
    let text = long["text"].as_str().unwrap();
    assert!(text.chars().count() < 60_200, "{}", text.chars().count());
    assert!(text.contains("truncated"), "marker missing");

    let unknown = call_err(
        &client,
        "get_item",
        json!({"id": "01NOPEXXXXXXXXXXXXXXXXXXXX"}),
    )
    .await;
    assert!(unknown.starts_with("not_found:"), "{unknown}");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn list_buckets_and_items() {
    let dir = tempfile::tempdir().unwrap();
    let db = Stow::open(dir.path()).unwrap().db().clone();
    let b = buckets::create(&db, "Reading", Some("📚")).unwrap();
    let id = seed_item(&db, "https://example.com/okapi", "Okapi", Some(&b.id));
    let done_id = seed_item(&db, "https://example.com/done", "Done", Some(&b.id));
    items::set_done(&db, &done_id, true).unwrap();
    let inbox_id = seed_item(&db, "https://example.com/inbox", "Loose", None);
    let client = connect(dir.path(), false).await;

    let list = call(&client, "list_buckets", json!({})).await;
    assert_eq!(list["inbox_count"], 1);
    let list = list["buckets"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"], "Reading");
    assert_eq!(list[0]["emoji"], "📚");
    assert_eq!(list[0]["item_count"], 2);
    assert_eq!(list[0]["open_count"], 1);

    let inbox = call(&client, "list_items", json!({})).await;
    assert_eq!(
        inbox["items"].as_array().unwrap().len(),
        1,
        "inbox by default"
    );
    assert_eq!(inbox["items"][0]["id"], inbox_id);
    assert_eq!(inbox["items"][0]["bucket"], "Inbox");
    assert!(inbox["items"][0]["saved_at"].is_string());
    let by_name = call(&client, "list_items", json!({"bucket": "reading"})).await;
    assert_eq!(
        by_name["items"].as_array().unwrap().len(),
        1,
        "archived hidden"
    );
    assert_eq!(by_name["items"][0]["id"], id);
    assert_eq!(by_name["items"][0]["bucket"], "Reading");
    assert_eq!(by_name["total"], 1);
    let with_done = call(
        &client,
        "list_items",
        json!({"bucket": b.id, "include_archived": true}),
    )
    .await;
    assert_eq!(with_done["items"].as_array().unwrap().len(), 2);
    assert_eq!(with_done["items"][0]["archived"], true);
    let all = call(&client, "list_items", json!({"bucket": "all", "limit": 1})).await;
    assert_eq!(all["items"].as_array().unwrap().len(), 1);
    assert_eq!(all["total"], 2);
    let missing = call_err(&client, "list_items", json!({"bucket": "Nope"})).await;
    assert!(missing.starts_with("not_found:"), "{missing}");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn add_links_creates_and_dedupes() {
    let dir = tempfile::tempdir().unwrap();
    let client = connect(dir.path(), false).await;
    let bucket = call(
        &client,
        "create_bucket",
        json!({"name": "Later", "emoji": "⏳"}),
    )
    .await;
    assert_eq!(bucket["name"], "Later");
    let bucket_id = bucket["id"].as_str().unwrap().to_string();

    let out = call(
        &client,
        "add_links",
        json!({
            "urls": [
                "https://example.com/a",
                "https://example.com/a?utm_source=news",
                "https://youtu.be/abc123",
                "chrome://settings"
            ],
            "bucket": "later",
            "fetch": false
        }),
    )
    .await;
    let results = out["results"].as_array().unwrap();
    assert_eq!(results.len(), 4);
    assert_eq!(results[0]["created"], true);
    assert_eq!(results[1]["created"], false, "same canonical url");
    assert_eq!(results[1]["id"], results[0]["id"]);
    assert_eq!(results[0]["bucket_id"], bucket_id);
    assert_eq!(results[2]["created"], true);
    assert!(results[3]["error"]
        .as_str()
        .unwrap()
        .starts_with("invalid_url:"));
    assert!(results[3].get("id").is_none());

    let db = Stow::open(dir.path()).unwrap().db().clone();
    let all = items::list(&db, items::ItemFilter::All, true).unwrap();
    assert_eq!(all.len(), 2);
    let article = items::get(&db, results[0]["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        article.snapshot_status,
        JobStatus::Pending,
        "fetch=false leaves the snapshot for the app to resume"
    );
    let video = items::get(&db, results[2]["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        video.snapshot_status,
        JobStatus::None,
        "videos get no snapshot"
    );

    // A second call with the same URL in the Inbox is a no-op (`created: false`).
    let again = call(
        &client,
        "add_links",
        json!({"urls": ["https://example.com/a"], "fetch": false}),
    )
    .await;
    assert_eq!(again["results"][0]["created"], false);
    assert_eq!(
        items::list(&db, items::ItemFilter::All, true)
            .unwrap()
            .len(),
        2
    );
    let bad_bucket = call_err(
        &client,
        "add_links",
        json!({"urls": ["https://example.com/z"], "bucket": "Nope", "fetch": false}),
    )
    .await;
    assert!(bad_bucket.starts_with("not_found:"), "{bad_bucket}");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn move_archive_and_notes() {
    let dir = tempfile::tempdir().unwrap();
    let db = Stow::open(dir.path()).unwrap().db().clone();
    let b = buckets::create(&db, "Work", None).unwrap();
    let id = seed_item(&db, "https://example.com/m", "Movable", None);
    let client = connect(dir.path(), false).await;

    let moved = call(&client, "move_item", json!({"id": id, "bucket": "work"})).await;
    assert_eq!(moved["bucket_id"], b.id);
    let back = call(&client, "move_item", json!({"id": id, "bucket": null})).await;
    assert!(back["bucket_id"].is_null());
    let back = call(&client, "move_item", json!({"id": id})).await;
    assert!(back["bucket_id"].is_null());

    let archived = call(&client, "archive_item", json!({"id": id, "archived": true})).await;
    assert!(archived["done_at"].is_string());
    let open = call(
        &client,
        "archive_item",
        json!({"id": id, "archived": false}),
    )
    .await;
    assert!(open["done_at"].is_null());

    let n = call(
        &client,
        "update_notes",
        json!({"id": id, "mode": "replace", "markdown": "first"}),
    )
    .await;
    assert_eq!(n["notes_md"], "first");
    let n = call(
        &client,
        "update_notes",
        json!({"id": id, "mode": "append", "markdown": "second"}),
    )
    .await;
    assert_eq!(n["notes_md"], "first\n\nsecond");
    let bn = call(
        &client,
        "update_notes",
        json!({"bucket_id": b.id, "mode": "append", "markdown": "bucket note"}),
    )
    .await;
    assert_eq!(bn["notes_md"], "bucket note");
    let neither = call_err(
        &client,
        "update_notes",
        json!({"mode": "append", "markdown": "x"}),
    )
    .await;
    assert!(neither.starts_with("invalid_input:"), "{neither}");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn tasks_reminders_export_doctor_delete() {
    let dir = tempfile::tempdir().unwrap();
    let db = Stow::open(dir.path()).unwrap().db().clone();
    let b = buckets::create(&db, "Work", None).unwrap();
    let id = seed_item(&db, "https://example.com/t", "Tasked", Some(&b.id));
    let client = connect(dir.path(), false).await;

    let task = call(
        &client,
        "add_task",
        json!({"parent_type": "item", "parent_id": id, "title": "Skim"}),
    )
    .await;
    assert_eq!(task["title"], "Skim");
    let task_id = task["id"].as_str().unwrap().to_string();
    let done = call(
        &client,
        "complete_task",
        json!({"id": task_id, "done": true}),
    )
    .await;
    assert!(done["done_at"].is_string());
    let done_again = call(
        &client,
        "complete_task",
        json!({"id": task_id, "done": true}),
    )
    .await;
    assert_eq!(done_again["done_at"], done["done_at"], "idempotent");
    let undone = call(
        &client,
        "complete_task",
        json!({"id": task_id, "done": false}),
    )
    .await;
    assert!(undone["done_at"].is_null());
    let bucket_task = call(
        &client,
        "add_task",
        json!({"parent_type": "bucket", "parent_id": b.id, "title": "Plan"}),
    )
    .await;
    assert_eq!(bucket_task["parent_type"], "bucket");

    let rem = call(
        &client,
        "add_reminder",
        json!({"target_type": "task", "target_id": task_id, "at": "2030-05-01T08:00:00+02:00", "message": "go"}),
    )
    .await;
    assert_eq!(rem["fire_at"], "2030-05-01T06:00:00.000000Z");
    let bad_time = call_err(
        &client,
        "add_reminder",
        json!({"target_type": "item", "target_id": id, "at": "tomorrow"}),
    )
    .await;
    assert!(bad_time.starts_with("invalid_input:"), "{bad_time}");

    let dest = tempfile::tempdir().unwrap();
    let exported = call(
        &client,
        "export",
        json!({"format": "json", "dest_dir": dest.path()}),
    )
    .await;
    assert_eq!(exported["items"], 1);
    assert!(Path::new(exported["path"].as_str().unwrap()).is_file());

    let doctor = call(&client, "doctor", json!({})).await;
    assert_eq!(doctor["integrity"], json!(["ok"]));
    assert_eq!(doctor["counts"]["items"], 1);
    assert_eq!(doctor["ok"], true);

    call(&client, "delete_item", json!({"id": id})).await;
    assert!(items::get(&db, &id).is_err());
    let gone = call_err(&client, "delete_item", json!({"id": id})).await;
    assert!(gone.starts_with("not_found:"), "{gone}");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn read_only_mode_rejects_mutations() {
    let dir = tempfile::tempdir().unwrap();
    let db = Stow::open(dir.path()).unwrap().db().clone();
    let id = seed_item(&db, "https://example.com/ro", "Read only", None);
    let client = connect(dir.path(), true).await;

    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(names, READ_TOOLS);

    let found = call(&client, "search", json!({"query": "read"})).await;
    assert_eq!(found["hits"][0]["item_id"], id);

    for name in WRITE_TOOLS {
        let r = try_call(&client, name, json!({})).await;
        assert!(r.is_err(), "{name} must be refused in read-only mode");
    }
    assert!(items::get(&db, &id).is_ok());
    assert_eq!(
        items::list(&db, items::ItemFilter::All, true)
            .unwrap()
            .len(),
        1
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn resources_render_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let db = Stow::open(dir.path()).unwrap().db().clone();
    let b = buckets::create(&db, "Garden", Some("🌱")).unwrap();
    buckets::update(
        &db,
        &b.id,
        &buckets::BucketPatch {
            description_md: Some("Things to plant.".into()),
            notes_md: Some("Order seeds in March.".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let id = seed_item(
        &db,
        "https://example.com/tomato",
        "Tomato guide",
        Some(&b.id),
    );
    items::set_notes(&db, &id, "Start indoors.").unwrap();
    tasks::create(&db, ParentType::Item, &id, "Buy pots").unwrap();
    write_snapshot(&db, &id, "Tomato guide", "Plant after the last frost.");
    let client = connect(dir.path(), false).await;

    let listed = client.list_resources(None).await.unwrap();
    let uris: Vec<&str> = listed.resources.iter().map(|r| r.uri.as_str()).collect();
    assert_eq!(uris, vec![format!("stow://bucket/{}", b.id)]);
    assert_eq!(listed.resources[0].name, "Garden");
    let templates = client.list_resource_templates(None).await.unwrap();
    let mut t: Vec<&str> = templates
        .resource_templates
        .iter()
        .map(|t| t.uri_template.as_str())
        .collect();
    t.sort_unstable();
    assert_eq!(t, vec!["stow://bucket/{id}", "stow://item/{id}"]);

    let read = |uri: String| {
        let client = &client;
        async move {
            let r = client
                .read_resource(ReadResourceRequestParams::new(uri))
                .await
                .unwrap();
            match &r.contents[0] {
                ResourceContents::TextResourceContents {
                    text, mime_type, ..
                } => {
                    assert_eq!(mime_type.as_deref(), Some("text/markdown"));
                    text.clone()
                }
                other => panic!("{other:?}"),
            }
        }
    };
    let item_md = read(format!("stow://item/{id}")).await;
    for needle in [
        "# Tomato guide",
        "https://example.com/tomato",
        "Garden",
        "## Notes\n\nStart indoors.",
        "- [ ] Buy pots",
        "Plant after the last frost.",
    ] {
        assert!(
            item_md.contains(needle),
            "missing {needle:?} in:\n{item_md}"
        );
    }
    let bucket_md = read(format!("stow://bucket/{}", b.id)).await;
    for needle in [
        "# 🌱 Garden",
        "Things to plant.",
        "Order seeds in March.",
        "Tomato guide",
        &format!("stow://item/{id}"),
    ] {
        assert!(
            bucket_md.contains(needle),
            "missing {needle:?} in:\n{bucket_md}"
        );
    }

    for bad in [
        "stow://item/01NOPEXXXXXXXXXXXXXXXXXXXX".to_string(),
        "stow://bucket/01NOPEXXXXXXXXXXXXXXXXXXXX".to_string(),
        "stow://nope/1".to_string(),
        "file:///etc/passwd".to_string(),
    ] {
        assert!(
            client
                .read_resource(ReadResourceRequestParams::new(bad.clone()))
                .await
                .is_err(),
            "{bad}"
        );
    }
    client.cancel().await.unwrap();
}

#[test]
fn read_only_env_is_parsed_leniently() {
    for (v, want) in [
        ("1", true),
        ("true", true),
        ("YES", true),
        ("0", false),
        ("false", false),
        ("", false),
        ("maybe", false),
    ] {
        assert_eq!(stow_mcp::read_only_env(Some(v)), want, "{v:?}");
    }
    assert!(!stow_mcp::read_only_env(None));
}
