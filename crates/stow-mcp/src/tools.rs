//! The tools. Each returns its result as structured JSON (also serialized into the text
//! content); a [`StowError`] becomes a tool-level error whose text starts with the
//! stable [`StowError::code`], e.g. `not_found: item not found: 01…`.

use std::collections::HashMap;
use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ErrorData as McpError};
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use stow_core::export::{export_library, ExportFormat};
use stow_core::models::{ItemKind, JobStatus, ParentType, TargetType};
use stow_core::repo::items::ItemFilter;
use stow_core::repo::{buckets, items, reminders, search, tasks};
use stow_core::{doctor, CaptureRequest, StowError};

use crate::view::{resolve_bucket, ItemRow, ItemView};
use crate::StowMcp;

/// Default and cap for `limit` arguments.
const DEFAULT_LIMIT: usize = 50;

/// Arguments of `search`.
#[derive(Debug, Deserialize, JsonSchema)]
struct SearchParams {
    /// Words to look for; each is a prefix match and all must occur.
    query: String,
    /// Most hits to return (default 25).
    limit: Option<usize>,
}

/// Arguments of `list_items`.
#[derive(Debug, Deserialize, JsonSchema)]
struct ListItemsParams {
    /// Bucket id or name, `inbox` (the default), or `all` for every bucket.
    bucket: Option<String>,
    /// Also return items marked done/archived (default false).
    include_archived: Option<bool>,
    /// Most items to return, newest first (default 50).
    limit: Option<usize>,
}

/// Arguments of `get_item`.
#[derive(Debug, Deserialize, JsonSchema)]
struct GetItemParams {
    /// Item id (ULID).
    id: String,
    /// Include the readable text of the page (default true; up to 60,000 characters).
    include_text: Option<bool>,
}

/// Arguments of `add_links`.
#[derive(Debug, Deserialize, JsonSchema)]
struct AddLinksParams {
    /// http(s) URLs to save; a URL already in the library is returned with `created: false`.
    urls: Vec<String>,
    /// Bucket id or name to save into; omit or `inbox` for the Inbox.
    bucket: Option<String>,
    /// Download title, metadata and readable text now (default true). With false the
    /// snapshot is left pending for the Stow app to fetch later.
    fetch: Option<bool>,
}

/// Arguments of `move_item`.
#[derive(Debug, Deserialize, JsonSchema)]
struct MoveItemParams {
    /// Item id.
    id: String,
    /// Destination bucket id or name; omit, null, or `inbox` for the Inbox.
    bucket: Option<String>,
}

/// Arguments of `archive_item`.
#[derive(Debug, Deserialize, JsonSchema)]
struct ArchiveItemParams {
    /// Item id.
    id: String,
    /// true marks it done/archived, false reopens it.
    archived: bool,
}

/// Arguments of `delete_item`.
#[derive(Debug, Deserialize, JsonSchema)]
struct DeleteItemParams {
    /// Item id.
    id: String,
}

/// How `update_notes` combines the new text with the existing notes.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum NotesMode {
    /// Add after the existing notes (separated by a blank line).
    Append,
    /// Overwrite the existing notes.
    Replace,
}

/// Arguments of `update_notes`.
#[derive(Debug, Deserialize, JsonSchema)]
struct UpdateNotesParams {
    /// Item id (give exactly one of `id` and `bucket_id`).
    id: Option<String>,
    /// Bucket id, to edit a bucket's notes instead.
    bucket_id: Option<String>,
    /// `append` or `replace`.
    mode: NotesMode,
    /// The notes text (Markdown).
    markdown: String,
}

/// Owner of a task: a saved link or a bucket.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Owner {
    /// A saved link.
    Item,
    /// A bucket.
    Bucket,
}

impl From<Owner> for ParentType {
    fn from(o: Owner) -> Self {
        match o {
            Owner::Item => ParentType::Item,
            Owner::Bucket => ParentType::Bucket,
        }
    }
}

/// Arguments of `add_task`.
#[derive(Debug, Deserialize, JsonSchema)]
struct AddTaskParams {
    /// `item` or `bucket`.
    parent_type: Owner,
    /// Id of the item or bucket.
    parent_id: String,
    /// Task text.
    title: String,
}

/// Arguments of `complete_task`.
#[derive(Debug, Deserialize, JsonSchema)]
struct CompleteTaskParams {
    /// Task id.
    id: String,
    /// true ticks the task, false reopens it.
    done: bool,
}

/// What a reminder is attached to.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Target {
    /// A saved link.
    Item,
    /// A bucket.
    Bucket,
    /// A task.
    Task,
}

impl From<Target> for TargetType {
    fn from(t: Target) -> Self {
        match t {
            Target::Item => TargetType::Item,
            Target::Bucket => TargetType::Bucket,
            Target::Task => TargetType::Task,
        }
    }
}

/// Arguments of `add_reminder`.
#[derive(Debug, Deserialize, JsonSchema)]
struct AddReminderParams {
    /// `item`, `bucket`, or `task`.
    target_type: Target,
    /// Id of the target.
    target_id: String,
    /// When to remind, RFC 3339 (any offset; stored as UTC), e.g. `2026-12-01T09:00:00+01:00`.
    at: String,
    /// Optional note shown in the notification.
    message: Option<String>,
}

/// Arguments of `create_bucket`.
#[derive(Debug, Deserialize, JsonSchema)]
struct CreateBucketParams {
    /// Bucket name.
    name: String,
    /// Optional emoji shown before the name.
    emoji: Option<String>,
}

/// Export format.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Format {
    /// One folder per bucket, one `.md` file per item, attachments copied alongside.
    Markdown,
    /// One JSON file with every table.
    Json,
}

impl From<Format> for ExportFormat {
    fn from(f: Format) -> Self {
        match f {
            Format::Markdown => ExportFormat::Markdown,
            Format::Json => ExportFormat::Json,
        }
    }
}

/// Arguments of `export`.
#[derive(Debug, Deserialize, JsonSchema)]
struct ExportParams {
    /// `markdown` or `json`.
    format: Format,
    /// Directory to write into (created if missing; an existing export is never overwritten).
    dest_dir: PathBuf,
}

/// One entry of the `add_links` result.
#[derive(Debug, Serialize)]
struct AddedLink {
    /// The URL as given.
    url: String,
    /// Item id, when the URL was saved or already known.
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    /// true when this call saved it, false when it was already in the library.
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<bool>,
    /// Title after the fetch (or the host name).
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    /// Bucket the item sits in; `null` for the Inbox (or when nothing was saved).
    bucket_id: Option<String>,
    /// Readable-snapshot state after this call.
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_status: Option<JobStatus>,
    /// `code: message` when the URL could not be saved at all.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// `code: message` when the item was saved but its metadata fetch failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    fetch_error: Option<String>,
}

fn error_text(e: &StowError) -> String {
    format!("{}: {e}", e.code())
}

/// Serialize `r` as the structured result, or map the error to a tool-level error.
fn done<T: Serialize>(r: Result<T, StowError>) -> Result<CallToolResult, McpError> {
    match r {
        Ok(v) => {
            let value = serde_json::to_value(v)
                .map_err(|e| McpError::internal_error(format!("serializing result: {e}"), None))?;
            Ok(CallToolResult::structured(value))
        }
        Err(e) => {
            let mut result = CallToolResult::error(vec![ContentBlock::text(error_text(&e))]);
            result.structured_content = Some(json!({
                "code": e.code(),
                "message": e.to_string(),
            }));
            Ok(result)
        }
    }
}

fn limit(n: Option<usize>, default: usize) -> usize {
    n.filter(|n| *n > 0).unwrap_or(default)
}

#[tool_router(vis = "pub(crate)")]
impl StowMcp {
    #[tool(
        description = "Full-text search across titles, notes, page text, transcripts and AI notes. Returns hits with doc_type (bucket | item | snapshot | recording | ai_run), item_id, bucket_id, title and a [bracketed] snippet.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    fn search(&self, Parameters(p): Parameters<SearchParams>) -> Result<CallToolResult, McpError> {
        done(
            search::query(self.db(), &p.query, limit(p.limit, 25))
                .map(|hits| json!({ "hits": hits })),
        )
    }

    #[tool(
        description = "List the buckets (name, emoji, description, item_count) in sidebar order.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    fn list_buckets(&self) -> Result<CallToolResult, McpError> {
        let db = self.db();
        done(buckets::list(db).and_then(|list| {
            let rows = list
                .into_iter()
                .map(|b| {
                    let n = items::count(db, &ItemFilter::Bucket(b.id.clone()), true)?;
                    let open = items::count(db, &ItemFilter::Bucket(b.id.clone()), false)?;
                    Ok(json!({
                        "id": b.id,
                        "name": b.name,
                        "emoji": b.emoji,
                        "description_md": b.description_md,
                        "item_count": n,
                        "open_count": open,
                        "created_at": b.created_at,
                    }))
                })
                .collect::<Result<Vec<_>, StowError>>()?;
            let inbox = items::count(db, &ItemFilter::Inbox, true)?;
            Ok(json!({ "buckets": rows, "inbox_count": inbox }))
        }))
    }

    #[tool(
        description = "List items (id, title, url, kind, bucket, saved_at, archived), newest first. Defaults to the Inbox; pass a bucket id or name, or `all`.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    fn list_items(
        &self,
        Parameters(p): Parameters<ListItemsParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db();
        done((|| {
            let filter = match p.bucket.as_deref().map(str::trim) {
                Some(b) if b.eq_ignore_ascii_case("all") => ItemFilter::All,
                other => resolve_bucket(db, other)?.map_or(ItemFilter::Inbox, ItemFilter::Bucket),
            };
            let include_done = p.include_archived.unwrap_or(false);
            let total = items::count(db, &filter, include_done)?;
            let mut names: HashMap<String, String> = HashMap::new();
            let rows = items::list(db, filter, include_done)?
                .into_iter()
                .take(limit(p.limit, DEFAULT_LIMIT))
                .map(|it| {
                    let name = match &it.bucket_id {
                        None => "Inbox".to_string(),
                        Some(id) => {
                            if !names.contains_key(id) {
                                names.insert(id.clone(), buckets::get(db, id)?.name);
                            }
                            names[id].clone()
                        }
                    };
                    Ok(ItemRow::new(it, &name))
                })
                .collect::<Result<Vec<_>, StowError>>()?;
            Ok(json!({ "items": rows, "total": total }))
        })())
    }

    #[tool(
        description = "Everything about one saved link: metadata, bucket, notes, tasks, reminders, the readable page text (Markdown, truncated at 60,000 characters), voice-memo transcripts and AI notes.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    fn get_item(
        &self,
        Parameters(p): Parameters<GetItemParams>,
    ) -> Result<CallToolResult, McpError> {
        done(ItemView::load(
            self.db(),
            &p.id,
            p.include_text.unwrap_or(true),
        ))
    }

    #[tool(
        description = "Save one or more URLs (deduplicated by canonical URL) into a bucket or the Inbox and, unless fetch is false, download their title, metadata and readable text now. Returns one entry per URL: id, created, title, or an error.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn add_links(
        &self,
        Parameters(p): Parameters<AddLinksParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db();
        let bucket_id = match resolve_bucket(db, p.bucket.as_deref()) {
            Ok(b) => b,
            Err(e) => return done(Err::<(), _>(e)),
        };
        let fetch = p.fetch.unwrap_or(true);
        let mut results = Vec::with_capacity(p.urls.len());
        for url in p.urls {
            results.push(self.add_one(url, bucket_id.clone(), fetch).await);
        }
        done(Ok(json!({ "results": results })))
    }

    #[tool(
        description = "Move an item into a bucket (id or name); omit the bucket or pass `inbox` to move it to the Inbox.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn move_item(
        &self,
        Parameters(p): Parameters<MoveItemParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db();
        done(
            resolve_bucket(db, p.bucket.as_deref())
                .and_then(|b| items::move_to(db, &p.id, b.as_deref())),
        )
    }

    #[tool(
        description = "Mark an item done/archived (archived: true) or reopen it (false). Archived items stay searchable.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn archive_item(
        &self,
        Parameters(p): Parameters<ArchiveItemParams>,
    ) -> Result<CallToolResult, McpError> {
        done(items::set_done(self.db(), &p.id, p.archived))
    }

    #[tool(
        description = "Delete an item permanently, with its notes, tasks, reminders, attachments, recordings, AI notes and snapshot files.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn delete_item(
        &self,
        Parameters(p): Parameters<DeleteItemParams>,
    ) -> Result<CallToolResult, McpError> {
        done(items::delete(self.db(), &p.id).map(|()| json!({ "id": p.id, "deleted": true })))
    }

    #[tool(
        description = "Append to or replace the Markdown notes of an item (`id`) or a bucket (`bucket_id`).",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn update_notes(
        &self,
        Parameters(p): Parameters<UpdateNotesParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db();
        let combine = |existing: &str| match p.mode {
            NotesMode::Replace => p.markdown.clone(),
            NotesMode::Append if existing.trim().is_empty() => p.markdown.clone(),
            NotesMode::Append => format!("{}\n\n{}", existing.trim_end(), p.markdown),
        };
        match (&p.id, &p.bucket_id) {
            (Some(id), None) => done(
                items::get(db, id).and_then(|it| items::set_notes(db, id, &combine(&it.notes_md))),
            ),
            (None, Some(bucket_id)) => done(buckets::get(db, bucket_id).and_then(|b| {
                buckets::update(
                    db,
                    bucket_id,
                    &buckets::BucketPatch {
                        notes_md: Some(combine(&b.notes_md)),
                        ..Default::default()
                    },
                )
            })),
            _ => done(Err::<(), _>(StowError::invalid(
                "id",
                "give exactly one of `id` (item) and `bucket_id`",
            ))),
        }
    }

    #[tool(
        description = "Add a checklist task to an item or a bucket.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn add_task(
        &self,
        Parameters(p): Parameters<AddTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        done(tasks::create(
            self.db(),
            p.parent_type.into(),
            &p.parent_id,
            &p.title,
        ))
    }

    #[tool(
        description = "Tick a task off (done: true) or reopen it (false).",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn complete_task(
        &self,
        Parameters(p): Parameters<CompleteTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db();
        done(tasks::get(db, &p.id).and_then(|t| {
            if t.done_at.is_some() == p.done {
                Ok(t)
            } else {
                tasks::toggle(db, &p.id)
            }
        }))
    }

    #[tool(
        description = "Schedule a reminder (the Stow app shows a notification) on an item, bucket or task at an RFC 3339 time.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn add_reminder(
        &self,
        Parameters(p): Parameters<AddReminderParams>,
    ) -> Result<CallToolResult, McpError> {
        done(reminders::create(
            self.db(),
            p.target_type.into(),
            &p.target_id,
            &p.at,
            p.message.as_deref(),
        ))
    }

    #[tool(
        description = "Create a bucket (an intent-named pile of links) with an optional emoji.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn create_bucket(
        &self,
        Parameters(p): Parameters<CreateBucketParams>,
    ) -> Result<CallToolResult, McpError> {
        done(buckets::create(self.db(), &p.name, p.emoji.as_deref()))
    }

    #[tool(
        description = "Export the whole library into a directory as Markdown folders or one JSON file. Returns the path written and the bucket/item counts.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn export(&self, Parameters(p): Parameters<ExportParams>) -> Result<CallToolResult, McpError> {
        done(export_library(self.db(), p.format.into(), &p.dest_dir))
    }

    #[tool(
        description = "Health check of the library: SQLite integrity, search-index consistency, row counts and unfinished background jobs.",
        annotations(read_only_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    fn doctor(&self) -> Result<CallToolResult, McpError> {
        done(doctor::report(self.db()).and_then(|r| {
            let mut v = serde_json::to_value(&r)?;
            v["ok"] = json!(r.is_ok());
            Ok(v)
        }))
    }
}

impl StowMcp {
    /// Save one URL for `add_links`; errors land in the entry rather than failing the call.
    async fn add_one(&self, url: String, bucket_id: Option<String>, fetch: bool) -> AddedLink {
        let db = self.db();
        let mut out = AddedLink {
            url: url.clone(),
            id: None,
            created: None,
            title: None,
            bucket_id: None,
            snapshot_status: None,
            error: None,
            fetch_error: None,
        };
        let created = match self.stow.capture(CaptureRequest {
            url,
            bucket_id,
            title: None,
        }) {
            Ok(c) => c,
            Err(e) => {
                out.error = Some(error_text(&e));
                return out;
            }
        };
        let item = created.value;
        let fresh_page = created.created
            && item.kind != ItemKind::Video
            && item.snapshot_status == JobStatus::None;
        let after = if created.created && fetch {
            // Metadata and snapshot inline (no job queue in this process); the snapshot
            // outcome is recorded on the row either way.
            if let Err(e) = self.stow.fetch_item(&item.id).await {
                out.fetch_error = Some(error_text(&e));
            }
            items::get(db, &item.id)
        } else if fresh_page {
            // Leave the fetch to the Stow app: `pending` rows are resumed on its next launch.
            items::mark_snapshot_pending(db, &item.id)
        } else {
            Ok(item)
        };
        match after {
            Ok(it) => {
                out.id = Some(it.id);
                out.created = Some(created.created);
                out.title = it.title;
                out.bucket_id = it.bucket_id;
                out.snapshot_status = Some(it.snapshot_status);
            }
            Err(e) => out.error = Some(error_text(&e)),
        }
        out
    }
}
