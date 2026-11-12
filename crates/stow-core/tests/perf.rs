//! Load tests: a library of 5,000 items across 50 buckets, all with notes, must stay
//! responsive for listing, counting, searching, exporting, and backing up.
//!
//! Timings are printed with `--nocapture`. Each hot-path measurement takes the fastest of
//! three runs so one scheduler hiccup does not fail the build; a real regression slows
//! every run.

use std::path::Path;
use std::time::{Duration, Instant};

use stow_core::export::{backup_database, export_library, ExportFormat};
use stow_core::repo::items::ItemFilter;
use stow_core::repo::{buckets, items, search};
use stow_core::Db;

const BUCKETS: usize = 50;
const ITEMS: usize = 5_000;

const TOPICS: &[&str] = &[
    "rust",
    "sqlite",
    "tauri",
    "svelte",
    "whisper",
    "cooking",
    "cycling",
    "gardening",
    "astronomy",
    "chess",
];

fn fastest_of<T>(runs: usize, mut f: impl FnMut() -> T) -> (Duration, T) {
    let mut best: Option<(Duration, T)> = None;
    for _ in 0..runs {
        let t = Instant::now();
        let v = f();
        let d = t.elapsed();
        if best.as_ref().is_none_or(|(b, _)| d < *b) {
            best = Some((d, v));
        }
    }
    best.unwrap()
}

/// 50 buckets, 5,000 items (100 per bucket), every item with a note. Returns bucket ids.
fn seed(db: &Db) -> Vec<String> {
    let bucket_ids: Vec<String> = (0..BUCKETS)
        .map(|b| {
            buckets::create(db, &format!("Bucket {b:02}"), None)
                .unwrap()
                .id
        })
        .collect();
    for i in 0..ITEMS {
        let topic = TOPICS[i % TOPICS.len()];
        let it = items::create(
            db,
            items::NewItem {
                url: format!("https://example.com/{topic}/post-{i}?utm_source=perf"),
                bucket_id: Some(bucket_ids[i % BUCKETS].clone()),
                title: Some(format!("Guide {i}: notes on {topic}")),
            },
        )
        .unwrap();
        assert!(it.created, "item {i} deduped unexpectedly");
        items::set_notes(
            db,
            &it.value.id,
            &format!("Remember to read this {topic} guide; item number {i} of {ITEMS}."),
        )
        .unwrap();
    }
    bucket_ids
}

/// `(item .md files, README.md files)` under `root`, recursively.
fn count_markdown(root: &Path) -> (usize, usize) {
    let mut items = 0;
    let mut readmes = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().is_some_and(|n| n == "README.md") {
                readmes += 1;
            } else if p.extension().is_some_and(|e| e == "md") {
                items += 1;
            }
        }
    }
    (items, readmes)
}

#[test]
#[allow(clippy::too_many_lines)] // one seed of 5,000 items, every measurement against it
fn library_of_5000_items_stays_fast() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(dir.path()).unwrap();

    let t = Instant::now();
    let bucket_ids = seed(&db);
    let seed_time = t.elapsed();
    eprintln!("perf: seeded {ITEMS} items in {BUCKETS} buckets with notes in {seed_time:?}");

    // items::list for one bucket (100 rows) < 50 ms.
    let bucket = ItemFilter::Bucket(bucket_ids[7].clone());
    let (list_time, listed) = fastest_of(3, || items::list(&db, bucket.clone(), true).unwrap());
    eprintln!("perf: items::list(bucket) = {list_time:?}");
    assert_eq!(listed.len(), ITEMS / BUCKETS);
    assert!(
        listed
            .windows(2)
            .all(|w| (&w[0].created_at, &w[0].id) >= (&w[1].created_at, &w[1].id)),
        "newest first"
    );
    assert!(
        list_time < Duration::from_millis(50),
        "items::list took {list_time:?}"
    );

    // items::list of everything is still reasonable (not part of the budget, but it is
    // what `stw ls --all` does).
    let (all_time, all) = fastest_of(3, || items::list(&db, ItemFilter::All, true).unwrap());
    eprintln!("perf: items::list(all) = {all_time:?}");
    assert_eq!(all.len(), ITEMS);

    // items::count < 10 ms.
    let (count_time, n) = fastest_of(3, || items::count(&db, &ItemFilter::All, true).unwrap());
    eprintln!("perf: items::count(all) = {count_time:?}");
    assert_eq!(n, ITEMS);
    assert!(
        count_time < Duration::from_millis(10),
        "items::count took {count_time:?}"
    );
    let (count_b_time, nb) = fastest_of(3, || items::count(&db, &bucket, false).unwrap());
    assert_eq!(nb, ITEMS / BUCKETS);
    assert!(
        count_b_time < Duration::from_millis(10),
        "items::count took {count_b_time:?}"
    );

    // search::query for a word in every doc (title "Guide" + notes "guide") < 100 ms.
    let (search_time, hits) = fastest_of(3, || search::query(&db, "guide", 25).unwrap());
    eprintln!("perf: search::query(\"guide\", 25) = {search_time:?}");
    assert_eq!(hits.len(), 25, "limit honoured");
    assert!(
        search_time < Duration::from_millis(100),
        "search took {search_time:?}"
    );
    let again = search::query(&db, "guide", 25).unwrap();
    assert_eq!(again, hits, "ordering is stable");
    let (two_words_time, rust_hits) =
        fastest_of(3, || search::query(&db, "rust guide", 25).unwrap());
    eprintln!("perf: search::query(\"rust guide\", 25) = {two_words_time:?}");
    assert_eq!(rust_hits.len(), 25);
    assert!(
        rust_hits.iter().all(|h| h.title.contains("rust")),
        "every hit is a rust guide: {rust_hits:?}"
    );
    assert!(
        two_words_time < Duration::from_millis(100),
        "search took {two_words_time:?}"
    );
    // The whole index for the most common word.
    let (search_all_time, every) = fastest_of(3, || search::query(&db, "guide", 10_000).unwrap());
    eprintln!(
        "perf: search::query(\"guide\", 10000) -> {} hits in {search_all_time:?}",
        every.len()
    );
    assert_eq!(
        every.len(),
        ITEMS,
        "one item doc per item; buckets do not say 'guide'"
    );

    // export_library (Markdown) completes and writes one file per item + one README per bucket.
    let dest = tempfile::tempdir().unwrap();
    let t = Instant::now();
    let report = export_library(&db, ExportFormat::Markdown, dest.path()).unwrap();
    let export_time = t.elapsed();
    eprintln!("perf: export_library(markdown) = {export_time:?}");
    assert_eq!((report.buckets, report.items), (BUCKETS, ITEMS));
    let (md_files, readmes) = count_markdown(&report.path);
    assert_eq!((md_files, readmes), (ITEMS, BUCKETS));
    assert!(
        !report.path.join("Inbox").exists(),
        "no inbox items, no Inbox folder"
    );
    for b in 0..BUCKETS {
        let folder = report.path.join(format!("Bucket {b:02}"));
        assert!(folder.is_dir(), "{}", folder.display());
        let (n, r) = count_markdown(&folder);
        assert_eq!((n, r), (ITEMS / BUCKETS, 1), "{}", folder.display());
    }

    // JSON export too: the item count must be exact.
    let t = Instant::now();
    let json = export_library(&db, ExportFormat::Json, dest.path()).unwrap();
    eprintln!("perf: export_library(json) = {:?}", t.elapsed());
    assert_eq!(json.items, ITEMS);
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&json.path).unwrap()).unwrap();
    assert_eq!(v["tables"]["items"].as_array().unwrap().len(), ITEMS);

    // backup_database at this size: a consistent, openable copy with the FTS index.
    let copy_dir = dest.path().join("copy");
    let t = Instant::now();
    let bytes = backup_database(&db, &copy_dir.join("stow.db")).unwrap();
    let backup_time = t.elapsed();
    eprintln!("perf: backup_database = {backup_time:?} ({bytes} bytes)");
    assert!(
        bytes > 1024 * 1024,
        "{bytes} bytes is too small for 5,000 items"
    );
    let copy = Db::open(&copy_dir).unwrap();
    assert_eq!(items::count(&copy, &ItemFilter::All, true).unwrap(), ITEMS);
    assert_eq!(buckets::list(&copy).unwrap().len(), BUCKETS);
    assert_eq!(search::query(&copy, "guide", 10_000).unwrap().len(), ITEMS);
    assert_eq!(
        search::query(&copy, "chess", 10).unwrap().len(),
        10,
        "fts index came along"
    );
    assert!(
        seed_time + export_time + backup_time < Duration::from_secs(60),
        "whole test budget blown: seed {seed_time:?} export {export_time:?} backup {backup_time:?}"
    );
}
