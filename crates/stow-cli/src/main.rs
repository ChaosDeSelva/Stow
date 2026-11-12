//! `stw` — terminal front end over stow-core.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use stow_core::capture::normalize_url;
use stow_core::export::{self, ExportFormat};
use stow_core::models::{Bucket, Item};
use stow_core::repo::{buckets, items, search};
use stow_core::{doctor, CaptureRequest, Db, EntityKind, Stow, StowError};

/// Exit code for "the bucket or item you named does not exist".
const EXIT_NOT_FOUND: u8 = 3;

/// `-b inbox` (any case) means the Inbox, never a bucket of that name.
fn is_inbox(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case("inbox")
}

#[derive(Parser)]
#[command(name = "stw", version, about = "Park links in intent buckets")]
struct Cli {
    /// Data directory (default: ~/Library/Application Support/Stow)
    #[arg(long, env = "STOW_DATA_DIR", global = true)]
    data_dir: Option<PathBuf>,
    /// Also fetch localhost and private-network URLs (off: they are saved, not fetched)
    #[arg(long, env = "STOW_ALLOW_LOCAL_URLS", global = true)]
    allow_local_urls: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Save a URL (fetches metadata and a readable snapshot unless --no-fetch)
    Add {
        url: String,
        /// Bucket name; created if missing. Omit (or pass `inbox`) for the Inbox.
        #[arg(short, long)]
        bucket: Option<String>,
        /// Title to use instead of the fetched one
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        no_fetch: bool,
    },
    /// List items (Inbox by default)
    Ls {
        /// Bucket name, or `inbox`
        #[arg(short, long)]
        bucket: Option<String>,
        /// Every item, regardless of bucket
        #[arg(long)]
        all: bool,
        /// Print JSON instead of lines
        #[arg(long)]
        json: bool,
    },
    /// List buckets, or `buckets new <name>` to create one
    Buckets {
        /// Print JSON instead of lines
        #[arg(long)]
        json: bool,
        #[command(subcommand)]
        cmd: Option<BucketCmd>,
    },
    /// Move an item (id or unique prefix) to a bucket name, or `inbox`
    Mv { item: String, bucket: String },
    /// Full-text search across titles, notes, snapshots, transcripts
    Search {
        query: Vec<String>,
        /// Print JSON instead of lines
        #[arg(long)]
        json: bool,
        /// Rebuild the search index from the stored docs first (repairs a drifted index)
        #[arg(long)]
        rebuild_index: bool,
    },
    /// Delete an item (id or unique prefix) with its notes, tasks, files, and recordings
    Rm { item: String },
    /// Mark an item (id or unique prefix) done, or open again with --undo
    Done {
        item: String,
        #[arg(long)]
        undo: bool,
    },
    /// Print an item's URL; --browser opens it in the default browser
    Open {
        item: String,
        #[arg(long)]
        browser: bool,
    },
    /// Export the library into <dest>: Markdown folders or one JSON file
    Export {
        #[arg(long, value_enum, default_value = "markdown")]
        format: FormatArg,
        dest: PathBuf,
    },
    /// Copy the database to <file> (a consistent snapshot, safe while the app runs)
    Backup { file: PathBuf },
    /// Check the database and search index, and show counts and unfinished jobs
    Doctor {
        /// Print JSON instead of lines
        #[arg(long)]
        json: bool,
    },
    /// Serve the library to an MCP client (Claude Desktop, Claude Code) over stdio
    Mcp {
        /// Hide and refuse the tools that change the library (also `STOW_MCP_READ_ONLY=1`)
        #[arg(long)]
        read_only: bool,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum FormatArg {
    Markdown,
    Json,
}

#[derive(Subcommand)]
enum BucketCmd {
    New { name: String },
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            let not_found = e
                .downcast_ref::<StowError>()
                .is_some_and(StowError::is_not_found);
            ExitCode::from(if not_found { EXIT_NOT_FOUND } else { 1 })
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let dir = match cli.data_dir {
        Some(d) => d,
        None => stow_core::default_data_dir()
            .ok_or_else(|| anyhow!("no data directory on this platform; pass --data-dir"))?,
    };
    let stow = Stow::open(&dir)
        .with_context(|| format!("opening {}", dir.display()))?
        .with_allow_local_urls(cli.allow_local_urls);
    let db: &Db = stow.db();
    match cli.cmd {
        Cmd::Add {
            url,
            bucket,
            title,
            no_fetch,
        } => cmd_add(&stow, url, bucket, title, no_fetch),
        Cmd::Ls { bucket, all, json } => cmd_ls(db, bucket, all, json),
        Cmd::Buckets { cmd: None, json } => cmd_buckets(db, json),
        Cmd::Buckets {
            cmd: Some(BucketCmd::New { name }),
            json,
        } => {
            let b = buckets::create(db, &name, None)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&bucket_json(db, &b)?)?);
            } else {
                println!("{}", bucket_line(db, &b)?);
            }
            Ok(())
        }
        Cmd::Mv { item, bucket } => {
            let it = resolve_item(db, &item)?;
            let target = if is_inbox(&bucket) {
                None
            } else {
                Some(find_bucket(db, &bucket)?.id)
            };
            let moved = items::move_to(db, &it.id, target.as_deref())?;
            println!("{}", item_line(&moved));
            Ok(())
        }
        Cmd::Search {
            query,
            json,
            rebuild_index,
        } => {
            if rebuild_index {
                search::rebuild(db)?;
                println!("search index rebuilt");
            }
            if query.is_empty() {
                return Ok(());
            }
            cmd_search(db, &query.join(" "), json)
        }
        Cmd::Rm { item } => {
            let it = resolve_item(db, &item)?;
            items::delete(db, &it.id)?;
            println!("removed {}", item_line(&it));
            Ok(())
        }
        Cmd::Done { item, undo } => {
            let it = resolve_item(db, &item)?;
            let it = items::set_done(db, &it.id, !undo)?;
            println!("{}", item_line(&it));
            Ok(())
        }
        Cmd::Open { item, browser } => {
            let it = resolve_item(db, &item)?;
            println!("{}", it.url);
            if browser {
                open_in_browser(&it.url)?;
            }
            Ok(())
        }
        Cmd::Export { format, dest } => {
            let format = match format {
                FormatArg::Markdown => ExportFormat::Markdown,
                FormatArg::Json => ExportFormat::Json,
            };
            let r = export::export_library(db, format, &dest)?;
            println!(
                "exported {} buckets, {} items to {}",
                r.buckets,
                r.items,
                r.path.display()
            );
            Ok(())
        }
        Cmd::Backup { file } => {
            let bytes = export::backup_database(db, &file)?;
            println!("backed up {bytes} bytes to {}", file.display());
            Ok(())
        }
        Cmd::Doctor { json } => cmd_doctor(db, json),
        Cmd::Mcp { read_only } => cmd_mcp(stow, read_only || stow_mcp::read_only_from_env()),
    }
}

/// Run the MCP server on stdin/stdout until the client disconnects. stdout is the
/// protocol channel, so logs go to stderr (`RUST_LOG=stow_mcp=debug`).
fn cmd_mcp(stow: Stow, read_only: bool) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(stow_mcp::serve_stdio(stow, read_only))
        .context("mcp server")
}

/// Hand the URL to the OS (`open` on macOS, `xdg-open` elsewhere on unix, `start` on
/// Windows). Only http(s) URLs are ever stored, so this cannot launch a local program.
fn open_in_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    };
    let status = cmd.arg(url).status().context("launching the browser")?;
    if !status.success() {
        return Err(anyhow!("browser launcher exited with {status}"));
    }
    Ok(())
}

fn cmd_add(
    stow: &Stow,
    url: String,
    bucket: Option<String>,
    title: Option<String>,
    no_fetch: bool,
) -> Result<()> {
    let db = stow.db();
    // Validate before touching anything, so a bad URL never creates a bucket.
    let canonical = normalize_url(&url)?;
    // A URL already sitting in a bucket stays there: creating the named bucket for it
    // would leave an empty bucket behind.
    let stays_put =
        items::find_by_canonical(db, canonical.as_str())?.is_some_and(|it| it.bucket_id.is_some());
    let mut new_bucket: Option<String> = None;
    let bucket_id = match bucket.filter(|b| !is_inbox(b)) {
        Some(name) if !stays_put => {
            if let Some(b) = buckets::find_by_name(db, &name)? {
                Some(b.id)
            } else {
                let b = buckets::create(db, &name, None)?;
                new_bucket = Some(b.id.clone());
                Some(b.id)
            }
        }
        _ => None,
    };
    let created = match stow.capture(CaptureRequest {
        url,
        bucket_id,
        title,
    }) {
        Ok(c) => c,
        Err(e) => {
            // The add did not happen: do not leave the bucket made for it.
            if let Some(id) = new_bucket {
                if let Err(de) = buckets::delete(db, &id) {
                    eprintln!("warning: could not remove bucket created for the failed add: {de}");
                }
            }
            return Err(e.into());
        }
    };
    let item = created.value;
    if !created.created {
        // Metadata and snapshot were fetched when it was first saved; don't redo them.
        println!("{}  (already saved)", item_line(&item));
        return Ok(());
    }
    if !no_fetch {
        let rt = tokio::runtime::Runtime::new()?;
        if let Err(e) = rt.block_on(stow.fetch_item(&item.id)) {
            eprintln!("warning: metadata fetch failed: {e}");
        }
    }
    println!("{}", item_line(&items::get(db, &item.id)?));
    Ok(())
}

fn cmd_ls(db: &Db, bucket: Option<String>, all: bool, json: bool) -> Result<()> {
    let filter = if all {
        items::ItemFilter::All
    } else if let Some(name) = bucket.filter(|b| !is_inbox(b)) {
        items::ItemFilter::Bucket(find_bucket(db, &name)?.id)
    } else {
        items::ItemFilter::Inbox
    };
    let list = items::list(db, filter, true)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&list)?);
    } else {
        for it in &list {
            println!("{}", item_line(it));
        }
    }
    Ok(())
}

fn cmd_buckets(db: &Db, json: bool) -> Result<()> {
    let list = buckets::list(db)?;
    if json {
        let rows = list
            .iter()
            .map(|b| bucket_json(db, b))
            .collect::<Result<Vec<serde_json::Value>>>()?;
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        for b in &list {
            println!("{}", bucket_line(db, b)?);
        }
    }
    Ok(())
}

fn cmd_search(db: &Db, query: &str, json: bool) -> Result<()> {
    let hits = search::query(db, query, 25)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&hits)?);
        return Ok(());
    }
    for hit in hits {
        println!(
            "{:<9} {:<26} {}  {}",
            hit.doc_type,
            hit.item_id.or(hit.bucket_id).unwrap_or_default(),
            hit.title,
            hit.snippet.replace('\n', " ")
        );
    }
    Ok(())
}

fn cmd_doctor(db: &Db, json: bool) -> Result<()> {
    let r = doctor::report(db)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&r)?);
    } else {
        println!("schema version: {}", r.schema_version);
        println!("integrity: {}", r.integrity.join("; "));
        match &r.fts_error {
            None => println!("search index: ok"),
            Some(e) => println!("search index: {e}  (run `stw search --rebuild-index`)"),
        }
        let c = &r.counts;
        println!(
            "buckets: {}  items: {}  tasks: {}  reminders: {}  attachments: {}  recordings: {}  \
             ai runs: {}  search docs: {}",
            c.buckets,
            c.items,
            c.tasks,
            c.reminders,
            c.attachments,
            c.recordings,
            c.ai_runs,
            c.search_docs
        );
        let j = &r.resume;
        println!(
            "unfinished jobs: snapshots {} pending / {} running, transcripts {} pending / {} running, \
             ai runs {} pending / {} running",
            j.snapshot_pending,
            j.snapshot_running,
            j.transcript_pending,
            j.transcript_running,
            j.ai_pending,
            j.ai_running
        );
    }
    if r.is_ok() {
        Ok(())
    } else {
        Err(anyhow!("doctor found problems"))
    }
}

fn find_bucket(db: &Db, name: &str) -> Result<Bucket> {
    Ok(buckets::find_by_name(db, name)?
        .ok_or_else(|| StowError::not_found(EntityKind::Bucket, name))?)
}

fn resolve_item(db: &Db, id_or_prefix: &str) -> Result<Item> {
    match items::get(db, id_or_prefix) {
        Ok(it) => return Ok(it),
        Err(e) if !e.is_not_found() => return Err(e.into()),
        Err(_) => {}
    }
    let mut matches = items::find_by_prefix(db, id_or_prefix)?;
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(StowError::not_found(EntityKind::Item, id_or_prefix).into()),
        _ => Err(anyhow!(
            "several items match {id_or_prefix:?}; give more characters"
        )),
    }
}

fn item_line(it: &Item) -> String {
    format!(
        "{}  {:<7}  {}  {}",
        it.id,
        it.kind.as_str(),
        it.title.as_deref().unwrap_or("-"),
        it.url
    )
}

fn item_count(db: &Db, b: &Bucket) -> Result<usize> {
    Ok(items::count(
        db,
        &items::ItemFilter::Bucket(b.id.clone()),
        true,
    )?)
}

fn bucket_line(db: &Db, b: &Bucket) -> Result<String> {
    let n = item_count(db, b)?;
    let emoji = b
        .emoji
        .as_deref()
        .map(|e| format!("{e} "))
        .unwrap_or_default();
    Ok(format!("{}  {emoji}{}  ({n} items)", b.id, b.name))
}

fn bucket_json(db: &Db, b: &Bucket) -> Result<serde_json::Value> {
    let mut v = serde_json::to_value(b)?;
    v["item_count"] = item_count(db, b)?.into();
    Ok(v)
}
