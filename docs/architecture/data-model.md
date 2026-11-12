# Data model

What is on disk, how the tables relate, how search works, and how the schema moves forward. Read from `crates/stow-core/migrations/*.sql`, `src/db.rs`, `src/repo/*.rs`, `src/export.rs`, and `src/models.rs`.

## File layout

The data directory is `~/Library/Application Support/Stow` (`stow_core::default_data_dir`), or `STOW_DATA_DIR` / `stw --data-dir`. `Db::open` creates it with mode `0700` on unix and makes the subfolders.

| Path | Contents | Written by |
|---|---|---|
| `stow.db` (+ `-wal`, `-shm`) | SQLite database, WAL mode | everything |
| `snapshots/<item-id>.md`, `.html` | readable copy of a saved page | snapshot job |
| `attachments/<attachment-id>/<original filename>` | user files, copied in | `attachments::add` |
| `audio/<recording-id>.wav` (`tmp-<id>.wav` while recording) | 16 kHz mono recordings | `stop_recording` |
| `models/ggml-<name>.bin` (`.part` while downloading) | Whisper models | transcription job / **Download now** |
| `assets/<item-id>-favicon.<ext>`, `<item-id>-image.<ext>` | favicons and preview images | metadata job |

API keys are not in this folder: Keychain service `io.github.chaosdeselva.stow`, accounts `anthropic` / `openai` (keys under the v0.1 service name `stow` are migrated on first read).

Connection pragmas: `journal_mode = WAL`, `foreign_keys = ON`, `busy_timeout = 5000`, `synchronous = NORMAL`, `temp_store = MEMORY`. One connection behind a mutex per process; the app and `stw` can both be open (WAL plus the busy timeout; covered by `concurrent_cli_and_app_writers_do_not_corrupt`). `Db::tx` uses `BEGIN IMMEDIATE`.

## Conventions

- Ids are ULIDs (26 Crockford base32 characters), time-sortable. Commands validate every id (`invalid_input` otherwise).
- Timestamps are RFC 3339 UTC strings in canonical form (`ids::now()`), so string comparison orders correctly.
- Enum-like columns are `TEXT` with a `CHECK`: `kind ∈ video|article|other`, job statuses `none|pending|running|done|failed`, `parent_type ∈ bucket|item`, `target_type ∈ bucket|item|task`, AI `action` and `provider`. `models.rs` mirrors each with a `str_enum!` that serializes the same string to JSON and SQLite.
- Field names in the Rust structs, the SQL columns, and the JSON the frontend sees are identical (snake_case).

## Tables

**buckets** — `id`, `name`, `emoji`, `description_md`, `notes_md`, `position` (sidebar order), `created_at`, `updated_at`, `archived_at`. `buckets::list` and `find_by_name` skip archived buckets; `get` does not. (Bucket archiving exists in core; the UI deletes buckets instead.)

**items** — `id`, `bucket_id` (`REFERENCES buckets ON DELETE SET NULL`; `NULL` = Inbox), `url` (as captured), `canonical_url` (`UNIQUE`; the dedupe key), `title`, `description`, `site_name`, `kind`, `favicon_path`, `image_path`, `notes_md`, `snapshot_status`, `snapshot_error`, `snapshot_fetched_at`, `snapshot_attempts` (v2), `done_at` (v2; the UI calls this **Archive**), `position` (unused by the UI), `created_at`, `updated_at`. Indexes: `items_canonical_url` (unique), `items_bucket_created (bucket_id, created_at DESC)`, `items_snapshot_status`.

**tasks** — `id`, `parent_type`, `parent_id`, `title`, `done_at`, `position` (1-based within the parent), `created_at`. Index `tasks_parent (parent_type, parent_id, position)`.

**reminders** — `id`, `target_type`, `target_id`, `fire_at`, `message`, `fired_at`, `snoozed_until` (overrides `fire_at` when set), `created_at`. Indexes: `reminders_fire_at`, `reminders_target`, and the partial `reminders_unfired_effective ON (COALESCE(snoozed_until, fire_at)) WHERE fired_at IS NULL` that the 30-second scheduler query uses. `snooze` sets `snoozed_until` and clears `fired_at` so the reminder is due again.

**attachments** — `id`, `parent_type`, `parent_id`, `filename`, `path` (absolute), `mime` (guessed from the name), `size_bytes`, `created_at`. Index `attachments_parent`.

**recordings** — `id`, `parent_type`, `parent_id`, `audio_path`, `duration_ms`, `transcript_md`, `transcript_status`, `transcript_error`, `transcript_attempts` (v2), `created_at`. Index `recordings_parent`.

**ai_runs** — `id`, `parent_type`, `parent_id`, `action`, `provider`, `model`, `input_hash` (hash of action + parent + context, recorded for reference), `output_md`, `status`, `error`, `created_at`. Index `ai_runs_parent`.

**settings** — `key TEXT PRIMARY KEY`, `value TEXT`. Known keys and defaults (`repo/settings.rs::Key`): `hotkey` = `Ctrl+Cmd+S`, `close_tab_after_capture` = `true`, `ai_provider` = `anthropic`, `ai_model` = `""` (provider default), `whisper_model` = `base.en`, `theme` = `light`, `extension_port` = `0`, `extension_token` = `""`, `onboarding_dismissed` = `false`, `onboarding_steps` = `{}` (JSON object of step → bool; steps `captured`, `automation`, `notifications`, `extension`, `ai_key`, `whisper`), `close_to_tray_explained`, `quit_on_close`, `open_at_login`, `ai_consent_given` = `false`. The app adds `allow_local_urls` (default unset = false) and `extension_id` (default `""` = any extension). Booleans are stored as the strings `true`/`false`. Secrets never go here except the extension bearer token, which the JSON export redacts.

**search_docs** and **search** — see below.

### Polymorphic parents

`tasks`, `attachments`, `recordings`, and `ai_runs` hang off a bucket *or* an item through `(parent_type, parent_id)`; `reminders` target a bucket, item, or task through `(target_type, target_id)`. SQLite cannot express that as a foreign key, so:

- Existence is checked in Rust where a wrong id would otherwise create a silent orphan: `items::create` and `move_to` check the bucket (`require_bucket`), and every command validates ids as ULIDs. Child inserts (`tasks::create`, `attachments::add`, …) trust the ULID check and the fact that the UI only offers existing parents.
- Row cleanup is done by `AFTER DELETE` triggers (v2), so it holds for the app, the CLI, and `sqlite3`:

| Trigger | Fires on | Does |
|---|---|---|
| `tasks_cascade` | `DELETE FROM tasks` | deletes the task's reminders |
| `recordings_cascade` | `DELETE FROM recordings` | deletes its `recording` search doc |
| `ai_runs_cascade` | `DELETE FROM ai_runs` | deletes its `ai_run` search doc |
| `items_cascade` | `DELETE FROM items` | deletes the item's tasks, attachments, recordings, AI runs, reminders, and `item`/`snapshot` search docs (the task and recording triggers then run for those rows) |
| `buckets_cascade` | `DELETE FROM buckets` | deletes the bucket's own tasks, attachments, recordings, AI runs, reminders, and `bucket` search doc; sets `bucket_id = NULL` on the remaining search docs (the items themselves fall back to the Inbox through the real FK's `ON DELETE SET NULL`) |

Triggers cannot touch files. `items::delete` and `buckets::delete` collect the attachment directories and audio paths of the rows about to disappear (`OwnedFiles::collect`) inside the transaction, delete the parent row (the triggers do the rest), commit, and only then remove the files, plus `snapshots/<id>.*` and `assets/<id>-*` for items. [ADR-0006](adr/0006-polymorphic-parents-with-triggers.md) records the decision.

## Search

Two tables since v2:

- `search_docs (id INTEGER PRIMARY KEY, doc_type, doc_id, item_id, bucket_id, title, body, UNIQUE(doc_type, doc_id))` is an ordinary table that repo functions maintain explicitly: `search::index_conn` upserts a doc, `remove_conn` deletes one, `set_item_bucket_conn` re-points every doc of an item when it moves. Indexes on `item_id` and `bucket_id`.
- `search` is an **external-content FTS5** table (`content = 'search_docs'`, `content_rowid = 'id'`, `tokenize = 'porter unicode61'`) over `title` and `body`. Three triggers keep it in step with `search_docs`: `search_docs_ai` (insert), `search_docs_ad` (delete), `search_docs_au` (`AFTER UPDATE OF title, body`). `search::rebuild` runs `INSERT INTO search(search) VALUES ('rebuild')` to repair the index from the side table.

So: *which* documents exist is decided in Rust (because snapshot text, transcripts, and AI output arrive asynchronously and need a bucket id attached), and the *index* follows by trigger.

Document types and who writes them:

| `doc_type` | `doc_id` | title / body | written by |
|---|---|---|---|
| `bucket` | bucket id | name / description + notes | `buckets::create`, `update` |
| `item` | item id | title / description + notes + url | `items::create`, `set_notes`, `set_metadata`, `move_to` |
| `snapshot` | item id | page title / plain text of the readable copy | `items::finish_snapshot` |
| `recording` | recording id | "Recording" / transcript | `recordings::set_transcript` |
| `ai_run` | run id | action / output | `ai_runs::complete` |

Every doc carries `item_id` (when it belongs to an item) and `bucket_id`, so results can say where a hit lives and follow the item when it moves.

Querying (`search::query(db, q, limit)`): each whitespace-separated word becomes a quoted prefix token (`"word"*`, embedded quotes removed), so FTS5 operators in user input are inert; results are ordered by `bm25(search, 2.0, 1.0)` (title weighted 2×) with a `snippet(...)` of the body (`[match]` brackets, 12 tokens). The CLI asks for 25 hits, the app for 50.

## Migrations and `user_version`

`db.rs` holds `MIGRATIONS: &[&str]` (`include_str!` of `0001_init.sql`, `0002_v2.sql`). `PRAGMA user_version` is the number of migrations applied. `migrate()`:

1. Reads `user_version`. If it is 0 but a legacy `schema_migrations` table exists (the very first builds recorded versions there), its `MAX(version)` is used instead.
2. If `user_version` is **greater** than the number of migrations the binary knows, `Db::open` fails with `StowError::SchemaTooNew` (code `schema`). An old Stow never runs against a newer database.
3. Applies each pending migration in its own transaction, then sets `user_version = n`. Migration 2 additionally runs `normalize_reminder_times` in Rust, rewriting `fire_at` / `snoozed_until` / `fired_at` into canonical UTC (v1 stored whatever string the UI sent).

Migrations are append-only and never edited once shipped. `Db::open_at(dir, upto)` lets tests build a v1 database and upgrade it; `migration_chain_applies_from_empty_and_from_v1` asserts a fresh database and an upgraded one end with identical tables, indexes, and triggers.

**0001_init** — all eight tables, the original `items_bucket_position` index, and a contentless-style FTS5 `search` table with its own `doc_type`/`doc_id`/`item_id`/`bucket_id` columns.

**0002_v2** — `items.done_at`, `items.snapshot_attempts`, `recordings.transcript_attempts`; drops `items_bucket_position` for `items_bucket_created` and adds `items_snapshot_status` and `reminders_unfired_effective`; creates `search_docs`, copies the old `search` rows into it (filling `bucket_id` for item-owned docs), drops and recreates `search` as external-content, adds the three sync triggers and rebuilds; adds the five `*_cascade` triggers.

## Export formats

Both are produced by `export::export_library(db, format, dest_dir)` (Settings → Data, `stw export`, or the `export_library` command, which requires an absolute `destDir`). An existing export of the same name is never overwritten: ` 2`, ` 3`, … is appended.

**Markdown** → `<dest>/Stow export <YYYY-MM-DD>/`

```
Inbox/
  <title-slug>-<id6>.md
<Bucket name>/                       # emoji omitted; filesystem-unsafe characters → "-"; case-insensitive uniqueness
  README.md                          # "# <emoji> <name>", description, ## Notes, ## Tasks, ## Files
  files/<attachment filename>        # the bucket's own attachments
  <title-slug>-<id6>.md              # one per item
  <title-slug>-<id6>/files/<name>    # the item's attachments
```

Each item file starts with YAML front matter — `title`, `url`, `saved` (created_at), `kind`, `done` (true/false), `bucket` — then `# Title`, the description as a blockquote, and sections **Notes**, **Tasks** (`- [x]` / `- [ ]`), **Transcripts** (`### Recording <created_at>` each), **AI notes** (`### <action>` for finished runs), **Files** (links). Empty sections are omitted. Archived buckets that still hold items are exported too. `slug` keeps alphanumerics, lowercases, joins runs with `-`, caps at 60 bytes; `id6` is the last six characters of the ULID.

**JSON** → `<dest>/stow-export-<YYYY-MM-DD>.json`

```json
{
  "format": "stow-export",
  "version": 1,
  "schema_version": 2,
  "exported_at": "<RFC 3339 UTC>",
  "tables": {
    "buckets": [...], "items": [...], "tasks": [...], "reminders": [...],
    "attachments": [...], "recordings": [...], "ai_runs": [...], "settings": [...]
  }
}
```

Every row of every table in `rowid` order, column names as keys, ids preserved, blobs as lowercase hex. The `settings` table omits `extension_token`. File contents (snapshots, attachments, audio) are not embedded; paths are.

**Database backup** (`export::backup_database(db, dest)`, Settings → Data → **Back up database…**, `stw backup <file>`) uses SQLite's online backup API, so the copy is consistent while the app and CLI keep writing, includes WAL contents, creates missing parent folders, replaces an existing file, and returns the byte count. It keeps every setting including the extension token: it is a private copy, not a share. The copy opens with `Db::open` (the FTS index comes along).
