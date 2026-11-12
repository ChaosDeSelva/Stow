//! CLI integration tests: run the `stw` binary against a temp data dir.

use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The binary against `dir`, allowed to fetch the 127.0.0.1 wiremock servers.
fn stw(dir: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("stw").unwrap();
    c.env("STOW_DATA_DIR", dir)
        .env("STOW_ALLOW_LOCAL_URLS", "true");
    c
}

#[test]
fn buckets_new_then_list() {
    let dir = tempfile::tempdir().unwrap();
    stw(dir.path())
        .args(["buckets", "new", "Blog"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Blog"));
    stw(dir.path())
        .arg("buckets")
        .assert()
        .success()
        .stdout(predicate::str::contains("Blog"));
}

#[tokio::test(flavor = "multi_thread")]
async fn add_then_ls_shows_item_with_fetched_title() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/post"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("<html><head><title>Fetched Title</title></head><body><p>Body text long enough to be an article paragraph for readability purposes here.</p></body></html>", "text/html"))
        .mount(&server).await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let url = format!("{}/post", server.uri());
    let out = tokio::task::spawn_blocking({
        let d = dir.path().to_path_buf();
        move || {
            stw(&d)
                .args(["add", &url, "-b", "Reading"])
                .assert()
                .success()
                .stdout(predicate::str::contains("Fetched Title"))
                .stdout(predicate::str::contains("article"));
            stw(&d)
                .args(["ls", "-b", "Reading"])
                .assert()
                .success()
                .stdout(predicate::str::contains("Fetched Title"));
            stw(&d)
                .arg("ls")
                .assert()
                .success()
                .stdout(predicate::str::contains("Fetched Title").not());
        }
    })
    .await;
    out.unwrap();
}

#[test]
fn mv_to_inbox_and_back() {
    let dir = tempfile::tempdir().unwrap();
    stw(dir.path())
        .args(["add", "https://example.invalid/x", "-b", "A", "--no-fetch"])
        .assert()
        .success();
    let out = stw(dir.path()).args(["ls", "-b", "A"]).output().unwrap();
    let id = String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    stw(dir.path())
        .args(["mv", &id[..8], "inbox"])
        .assert()
        .success();
    stw(dir.path())
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("example.invalid"));
    stw(dir.path()).args(["mv", &id, "A"]).assert().success();
    stw(dir.path())
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("example.invalid").not());
}

#[test]
fn add_rejects_chrome_url() {
    let dir = tempfile::tempdir().unwrap();
    stw(dir.path())
        .args(["add", "chrome://extensions"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid url"));
}

#[test]
fn search_finds_title() {
    let dir = tempfile::tempdir().unwrap();
    stw(dir.path())
        .args([
            "add",
            "https://example.invalid/zebra-guide",
            "--no-fetch",
            "--title",
            "Zebra Guide",
        ])
        .assert()
        .success();
    stw(dir.path())
        .args(["search", "zebra"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Zebra Guide"));
}

#[tokio::test(flavor = "multi_thread")]
async fn add_existing_url_does_not_refetch() {
    let server = MockServer::start().await;
    // The first `add` fetches metadata and the snapshot (one GET each); the second `add`
    // of the same URL must not fetch at all.
    Mock::given(method("GET")).and(path("/post"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("<html><head><title>Fetched Once</title></head><body><p>Body text long enough to be an article paragraph for readability purposes here.</p></body></html>", "text/html"))
        .expect(2)
        .mount(&server).await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let url = format!("{}/post", server.uri());
    let d = dir.path().to_path_buf();
    let u = url.clone();
    tokio::task::spawn_blocking(move || {
        stw(&d)
            .args(["add", &u])
            .assert()
            .success()
            .stdout(predicate::str::contains("Fetched Once"))
            .stdout(predicate::str::contains("(already saved)").not());
    })
    .await
    .unwrap();
    let after_first = server.received_requests().await.unwrap().len();
    let d = dir.path().to_path_buf();
    tokio::task::spawn_blocking(move || {
        stw(&d)
            .args(["add", &url])
            .assert()
            .success()
            .stdout(predicate::str::contains("Fetched Once"))
            .stdout(predicate::str::contains("(already saved)"));
    })
    .await
    .unwrap();
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        after_first,
        "second add made HTTP requests"
    );
    // Exactly one item exists.
    let out = stw(dir.path()).args(["ls", "--all"]).output().unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap().lines().count(), 1);
}

#[test]
fn add_invalid_url_does_not_create_bucket() {
    let dir = tempfile::tempdir().unwrap();
    stw(dir.path())
        .args(["add", "chrome://extensions", "-b", "Ghost"])
        .assert()
        .failure();
    stw(dir.path())
        .arg("buckets")
        .assert()
        .success()
        .stdout(predicate::str::contains("Ghost").not());
}

#[test]
fn json_output_for_ls_buckets_and_search() {
    let dir = tempfile::tempdir().unwrap();
    stw(dir.path())
        .args([
            "add",
            "https://example.invalid/okapi",
            "-b",
            "Zoo",
            "--no-fetch",
            "--title",
            "Okapi facts",
        ])
        .assert()
        .success();
    let out = stw(dir.path())
        .args(["ls", "--all", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let items: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["title"], "Okapi facts");
    assert!(items[0]["done_at"].is_null());

    let out = stw(dir.path())
        .args(["buckets", "--json"])
        .output()
        .unwrap();
    let buckets: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(buckets[0]["name"], "Zoo");
    assert_eq!(buckets[0]["item_count"], 1);

    let out = stw(dir.path())
        .args(["search", "okapi", "--json"])
        .output()
        .unwrap();
    let hits: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(hits
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["doc_type"] == "item"));
}

#[test]
fn not_found_exits_with_code_3() {
    let dir = tempfile::tempdir().unwrap();
    stw(dir.path())
        .args(["ls", "-b", "Nope"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("Nope"));
    stw(dir.path())
        .args(["mv", "01ZZZZ", "inbox"])
        .assert()
        .code(3);
}

#[test]
fn add_localhost_is_saved_but_not_fetched() {
    let dir = tempfile::tempdir().unwrap();
    stw(dir.path())
        .env_remove("STOW_ALLOW_LOCAL_URLS")
        .args(["add", "http://127.0.0.1:9/admin"])
        .assert()
        .success();
    let out = stw(dir.path()).args(["ls", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[0]["snapshot_status"], "failed");
    assert_eq!(v[0]["snapshot_error"], "local address not fetched");
}

/// `stw add <url> --no-fetch` and return the new item's id.
fn add_item(dir: &std::path::Path, url: &str, title: &str) -> String {
    let out = stw(dir)
        .args(["add", url, "--no-fetch", "--title", title])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn ls_json(dir: &std::path::Path) -> serde_json::Value {
    let out = stw(dir).args(["ls", "--all", "--json"]).output().unwrap();
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn rm_deletes_item_and_unknown_is_exit_3() {
    let dir = tempfile::tempdir().unwrap();
    let id = add_item(dir.path(), "https://example.invalid/rm-me", "Remove me");
    stw(dir.path()).args(["rm", &id[..10]]).assert().success();
    assert_eq!(ls_json(dir.path()).as_array().unwrap().len(), 0);
    stw(dir.path()).args(["rm", &id]).assert().code(3);
}

#[test]
fn done_marks_and_undo_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let id = add_item(dir.path(), "https://example.invalid/finish", "Finish me");
    stw(dir.path()).args(["done", &id]).assert().success();
    assert!(ls_json(dir.path())[0]["done_at"].is_string());
    stw(dir.path())
        .args(["done", "--undo", &id])
        .assert()
        .success();
    assert!(ls_json(dir.path())[0]["done_at"].is_null());
    stw(dir.path()).args(["done", "01ZZZZZZ"]).assert().code(3);
}

#[test]
fn open_prints_url() {
    let dir = tempfile::tempdir().unwrap();
    let id = add_item(dir.path(), "https://example.invalid/read?x=1", "Read me");
    stw(dir.path())
        .args(["open", &id])
        .assert()
        .success()
        .stdout("https://example.invalid/read?x=1\n");
}

#[test]
fn export_markdown_and_json() {
    let dir = tempfile::tempdir().unwrap();
    add_item(
        dir.path(),
        "https://example.invalid/exported",
        "Exported Item",
    );
    let dest = tempfile::tempdir().unwrap();
    let out = stw(dir.path())
        .args(["export", "--format", "markdown"])
        .arg(dest.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("1 items"), "{stdout}");
    let root = std::fs::read_dir(dest.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(root.join("Inbox").is_dir(), "{}", root.display());

    let out = stw(dir.path())
        .args(["export", "--format", "json"])
        .arg(dest.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let json = std::fs::read_dir(dest.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .expect("json written");
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(json).unwrap()).unwrap();
    assert_eq!(v["tables"]["items"].as_array().unwrap().len(), 1);

    stw(dir.path())
        .args(["export", "--format", "pdf"])
        .arg(dest.path())
        .assert()
        .failure();
}

#[test]
fn add_waits_for_a_concurrent_write_transaction() {
    // The app holds a write transaction for ~1 s (one `BEGIN IMMEDIATE` on its own
    // connection) while `stw add --no-fetch` runs: busy_timeout must make the CLI wait
    // and succeed, never fail with "database is locked".
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let app_db = stow_core::Db::open(&path).unwrap();
    let hold = std::time::Duration::from_secs(1);
    let gate = Arc::new(Barrier::new(2));
    let g = gate.clone();
    let holder = std::thread::spawn(move || {
        app_db
            .tx(|t| {
                t.execute(
                    "INSERT INTO settings (key, value) VALUES ('held', 'yes')",
                    [],
                )?;
                g.wait();
                std::thread::sleep(hold);
                Ok(())
            })
            .unwrap();
    });
    gate.wait();
    let started = std::time::Instant::now();
    stw(&path)
        .args([
            "add",
            "https://example.invalid/locked",
            "--no-fetch",
            "--title",
            "Waited",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Waited"))
        .stderr(predicate::str::contains("locked").not());
    let waited = started.elapsed();
    holder.join().unwrap();
    assert!(
        waited >= hold.saturating_sub(std::time::Duration::from_millis(100)),
        "the CLI cannot have committed while the app held the write lock ({waited:?})"
    );
    assert!(waited < std::time::Duration::from_secs(5), "{waited:?}");
    let items = ls_json(&path);
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["title"], "Waited");
}

#[test]
fn newer_schema_database_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    add_item(dir.path(), "https://example.invalid/old", "Old");
    let db = stow_core::Db::open(dir.path()).unwrap();
    db.tx(|t| {
        t.execute_batch("PRAGMA user_version = 99")?;
        Ok(())
    })
    .unwrap();
    drop(db);
    for args in [
        vec!["ls", "--all"],
        vec!["buckets"],
        vec!["add", "https://example.invalid/new", "--no-fetch"],
    ] {
        stw(dir.path())
            .args(&args)
            .assert()
            .code(1)
            .stderr(predicate::str::contains("schema version 99"))
            .stderr(predicate::str::contains("newer than this build"))
            .stdout(predicate::str::is_empty());
    }
    // The refused commands did not "fix" the version by writing anything.
    let err = stow_core::Db::open(dir.path()).expect_err("still version 99");
    assert_eq!(err.code(), "schema");
}

#[test]
fn mv_with_ambiguous_prefix_fails_without_moving() {
    let dir = tempfile::tempdir().unwrap();
    let a = add_item(dir.path(), "https://example.invalid/one", "One");
    let b = add_item(dir.path(), "https://example.invalid/two", "Two");
    stw(dir.path())
        .args(["buckets", "new", "Target"])
        .assert()
        .success();
    // Every ULID minted today starts with "01": that prefix matches both items.
    assert!(a.starts_with("01") && b.starts_with("01"), "{a} {b}");
    stw(dir.path())
        .args(["mv", "01", "Target"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("several items match"))
        .stderr(predicate::str::contains("more characters"));
    let items = ls_json(dir.path());
    assert!(
        items
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["bucket_id"].is_null()),
        "nothing moved: {items}"
    );
    // The same ambiguity is an error for rm / done / open, and nothing changes.
    stw(dir.path()).args(["rm", "01"]).assert().code(1);
    stw(dir.path()).args(["done", "01"]).assert().code(1);
    stw(dir.path()).args(["open", "01"]).assert().code(1);
    let items = ls_json(dir.path());
    assert_eq!(items.as_array().unwrap().len(), 2);
    assert!(items
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["done_at"].is_null()));
    // A long-enough prefix works.
    stw(dir.path())
        .args(["mv", &a[..a.len() - 1], "Target"])
        .assert()
        .success();
    let out = stw(dir.path())
        .args(["ls", "-b", "Target", "--json"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["id"], a);
}

#[test]
fn add_200_urls_then_ls_all_json_lists_them_quickly() {
    let dir = tempfile::tempdir().unwrap();
    let t = std::time::Instant::now();
    for i in 0..200 {
        let url = format!("https://example.invalid/bulk/{i}?utm_source=cli");
        let bucket = format!("Bulk {}", i % 5);
        stw(dir.path())
            .args(["add", &url, "--no-fetch", "-b", &bucket])
            .assert()
            .success();
    }
    let add_time = t.elapsed();
    eprintln!("perf: 200 x `stw add --no-fetch` = {add_time:?}");

    let t = std::time::Instant::now();
    let out = stw(dir.path())
        .args(["ls", "--all", "--json"])
        .output()
        .unwrap();
    let ls_time = t.elapsed();
    eprintln!("perf: `stw ls --all --json` over 200 items = {ls_time:?}");
    assert!(out.status.success(), "{out:?}");
    let items: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = items.as_array().unwrap();
    assert_eq!(arr.len(), 200);
    let urls: std::collections::HashSet<&str> = arr
        .iter()
        .map(|i| i["canonical_url"].as_str().unwrap())
        .collect();
    assert_eq!(urls.len(), 200, "no duplicates");
    assert!(
        urls.iter().all(|u| !u.contains("utm_source")),
        "tracking params stripped"
    );
    assert!(
        ls_time < std::time::Duration::from_secs(2),
        "`stw ls --all --json` took {ls_time:?}"
    );

    let out = stw(dir.path())
        .args(["buckets", "--json"])
        .output()
        .unwrap();
    let buckets: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(buckets.as_array().unwrap().len(), 5);
    assert!(buckets
        .as_array()
        .unwrap()
        .iter()
        .all(|b| b["item_count"] == 40));
}

#[test]
fn backup_writes_openable_copy() {
    let dir = tempfile::tempdir().unwrap();
    add_item(dir.path(), "https://example.invalid/backed-up", "Backed Up");
    let dest = tempfile::tempdir().unwrap();
    let copy_dir = dest.path().join("copy");
    stw(dir.path())
        .arg("backup")
        .arg(copy_dir.join("stow.db"))
        .assert()
        .success()
        .stdout(predicate::str::contains("bytes"));
    // The copy is a working data dir.
    stw(&copy_dir)
        .args(["ls", "--all"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Backed Up"));
}

#[test]
fn add_to_inbox_does_not_create_bucket() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["inbox", "Inbox", "INBOX"] {
        stw(dir.path())
            .args([
                "add",
                &format!("https://example.invalid/{name}"),
                "-b",
                name,
                "--no-fetch",
            ])
            .assert()
            .success();
    }
    stw(dir.path())
        .arg("buckets")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
    let inbox = ls_json(dir.path());
    assert_eq!(inbox.as_array().unwrap().len(), 3);
    assert!(inbox
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["bucket_id"].is_null()));
    // `ls -b inbox` is the Inbox too.
    stw(dir.path())
        .args(["ls", "-b", "Inbox"])
        .assert()
        .success()
        .stdout(predicate::str::contains("example.invalid/inbox"));
    // Re-adding a URL that already sits in a bucket does not create the new bucket either.
    stw(dir.path())
        .args([
            "add",
            "https://example.invalid/kept",
            "-b",
            "Keep",
            "--no-fetch",
        ])
        .assert()
        .success();
    stw(dir.path())
        .args([
            "add",
            "https://example.invalid/kept",
            "-b",
            "Ghost",
            "--no-fetch",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("(already saved)"));
    stw(dir.path())
        .arg("buckets")
        .assert()
        .success()
        .stdout(predicate::str::contains("Keep"))
        .stdout(predicate::str::contains("Ghost").not());
}

#[test]
fn doctor_reports_ok() {
    let dir = tempfile::tempdir().unwrap();
    add_item(dir.path(), "https://example.invalid/healthy", "Healthy");
    stw(dir.path())
        .args(["buckets", "new", "Checkup"])
        .assert()
        .success();
    stw(dir.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("integrity: ok"))
        .stdout(predicate::str::contains("search index: ok"))
        .stdout(predicate::str::contains("schema version: 2"))
        .stdout(predicate::str::contains("items: 1"))
        .stdout(predicate::str::contains("buckets: 1"));
    let out = stw(dir.path()).args(["doctor", "--json"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["integrity"], serde_json::json!(["ok"]));
    assert!(v["fts_error"].is_null());
    assert_eq!(v["counts"]["items"], 1);
    assert_eq!(v["counts"]["buckets"], 1);
    assert_eq!(v["resume"]["snapshot_pending"], 0);
}

#[test]
fn search_rebuild_index_repairs_search() {
    let dir = tempfile::tempdir().unwrap();
    add_item(dir.path(), "https://example.invalid/okapi", "Okapi facts");
    // Wipe the FTS index behind the app's back; the content table still has the doc.
    let db = stow_core::Db::open(dir.path()).unwrap();
    db.tx(|t| {
        t.execute("INSERT INTO search (search) VALUES ('delete-all')", [])?;
        Ok(())
    })
    .unwrap();
    drop(db);
    stw(dir.path())
        .args(["search", "okapi"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Okapi").not());
    stw(dir.path())
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("search index: "))
        .stdout(predicate::str::contains("rebuild-index"));
    stw(dir.path())
        .args(["search", "--rebuild-index"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rebuilt"));
    stw(dir.path())
        .args(["search", "okapi"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Okapi facts"));
    stw(dir.path()).arg("doctor").assert().success();
}

#[test]
fn mcp_help_and_closed_stdin_exit_cleanly() {
    Command::cargo_bin("stw")
        .unwrap()
        .args(["mcp", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--read-only"))
        .stdout(predicate::str::contains("STOW_MCP_READ_ONLY"));
    // A client that connects and disconnects without a handshake (closed stdin) is a
    // normal session end: exit 0, nothing but protocol traffic on stdout.
    let dir = tempfile::tempdir().unwrap();
    stw(dir.path())
        .arg("mcp")
        .write_stdin("")
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}
