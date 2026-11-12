> Historical record of the initial build (2026-11-08). Superseded by the code, [docs/architecture/README.md](README.md) and the ADRs; kept for context.

# Stow — design spec

## Context

The author keeps dozens of Chrome tabs open as "read/watch later" piles, which eats memory and loses intent. Stow is a local-first macOS app that gets a tab out of Chrome in one motion and parks it in an intent-based bucket ("watch these 5 for the business", "sources for the blog post", "training to finish"). Each bucket and item carries notes, a task list, reminders that fire native notifications, attachments, audio recordings with transcripts, and bring-your-own-key AI summaries. Everything stays on the machine.

Early decisions:
- Tauri 2 desktop app, Rust backend, Svelte 5 + Vite frontend (not SvelteKit, not React).
- SQLite as source of truth, plain files for snapshots/attachments/audio. Workspace with `stow-core`, `stow-cli`, `stow-app`.
- Capture: global hotkey grabbing the front Chrome tab, paste, "import all Chrome tabs" behind a confirm button in Settings, Chrome extension last.
- Items store metadata plus an offline readable snapshot, plus user-attached md/html files, plus notes.
- Un-bucketed items live in an Inbox; items can be imported into a bucket or moved later. One bucket per item.
- Tasks on buckets and items; reminders on buckets, items, or tasks.
- Transcription via local whisper.cpp (`whisper-rs`).
- Working name: **Stow**.


## Spec

### Layout

```
<repo root>/
  Cargo.toml                      workspace: crates/stow-core, crates/stow-cli, apps/stow-app/src-tauri
  crates/stow-core/               all logic, no UI deps
  crates/stow-cli/                clap binary `stow`
  apps/stow-app/                  Tauri 2 project; src/ is Svelte 5 + Vite, src-tauri/ is the Rust shell
  docs/architecture/              this spec (design-spec.md) and the implementation plan
```

Data dir: `~/Library/Application Support/Stow/` containing `stow.db`, `snapshots/<item_id>.md` and `.html`, `attachments/<attachment_id>/<filename>`, `audio/<recording_id>.wav`, `models/ggml-base.en.bin`. Core takes the data dir as a parameter so tests use a temp dir.

### Data model (SQLite via `rusqlite` with `bundled` feature, migrations as numbered SQL files applied at open)

- `buckets(id TEXT pk, name, emoji, description_md, notes_md, position INT, created_at, updated_at, archived_at NULL)`
- `items(id TEXT pk, bucket_id NULL fk, url, canonical_url, title, description, site_name, kind TEXT CHECK in ('video','article','other'), favicon_path, image_path, notes_md, snapshot_status TEXT CHECK in ('none','pending','running','done','failed'), snapshot_error, snapshot_fetched_at, position INT, created_at, updated_at)`
- `tasks(id, parent_type CHECK in ('bucket','item'), parent_id, title, done_at NULL, position, created_at)`
- `reminders(id, target_type CHECK in ('bucket','item','task'), target_id, fire_at, message, fired_at NULL, snoozed_until NULL, created_at)`
- `attachments(id, parent_type, parent_id, filename, path, mime, size_bytes, created_at)`
- `recordings(id, parent_type, parent_id, audio_path, duration_ms, transcript_md, transcript_status, transcript_error, created_at)`
- `ai_runs(id, parent_type, parent_id, action CHECK in ('summarize_page','summarize_notes','summarize_recording','suggest_tasks','extract_metadata'), provider, model, input_hash, output_md, status, error, created_at)`
- `settings(key pk, value)` — never API keys. Keys live in macOS Keychain via the `keyring` crate under service `stow`, accounts `anthropic` and `openai`.
- FTS5 external-content table `search(doc_type, doc_id, title, body)` kept in sync by core repo functions (not triggers, so snapshot text and transcripts can be added when they arrive).

IDs are ULIDs (`ulid` crate) so they sort by creation time. Timestamps are RFC 3339 UTC strings.

### Core crate modules

- `db`: open, migrate, connection handling (single `Mutex<Connection>`; this is a one-user desktop app).
- `repo::{buckets,items,tasks,reminders,attachments,recordings,ai_runs,search,settings}`: plain functions taking `&Db`. Every mutation updates `updated_at` and the FTS row.
- `capture`: `normalize_url` (strip tracking params, canonicalize YouTube URLs), `classify_kind` (YouTube/Vimeo = video, else article).
- `fetch::metadata`: `reqwest` GET with a desktop UA, parse `<title>`, meta description, OpenGraph tags, favicon via `scraper`. For YouTube use the oEmbed endpoint for title/author/thumbnail. Returns `Metadata` struct; never fails the save, returns `Option`.
- `fetch::snapshot` (slice 2): readability extraction to clean HTML, then HTML→Markdown; write both files.
- `jobs`: a tiny in-process queue (tokio mpsc) for snapshot/transcribe/ai jobs with status writes to the relevant table. The app and CLI both own a runtime and drive it.
- `reminders::scheduler` (slice 3): `due_reminders(now)` pure query; firing is the app's job.
- `ai` (slice 4): `trait Provider { async fn complete(&self, system, user) -> Result<String> }`, `AnthropicProvider` (Messages API, default `claude-sonnet-5-5`), `OpenAiProvider`. Prompt builders per action.
- `audio` (slice 5): `cpal` capture to 16 kHz mono WAV via `hound`; `whisper-rs` transcription producing timestamped Markdown; model download on first use with progress.
- `chrome` (lives in `stow-app`, not core, since it shells out to `osascript`): `front_tab() -> (url, title)`, `close_front_tab()`, `all_tabs()`, `close_tabs(ids)`.
- Errors: `thiserror` enum `StowError`; Tauri commands map to `String` for the frontend.

### Tauri app

- Window: three panes. Left sidebar: Inbox, Today (due reminders), buckets list, Settings. Middle: item list for the selection with favicon, title, site, kind badge, snapshot status dot. Right: item detail with metadata header, tabs for Notes / Page (snapshot reader) / Tasks / Files / Recordings / AI.
- Bucket view: bucket notes, bucket tasks, bucket reminders above the item list.
- Quick capture panel: a second small always-on-top window opened by the global shortcut (default ⌘⇧S via `tauri-plugin-global-shortcut`). Shows URL and title from Chrome, a bucket picker with type-ahead and "New bucket…", a "Close tab in Chrome" checkbox (remembered). Enter saves, Esc cancels. If Chrome isn't frontmost or Automation permission is denied, the panel opens with an empty URL field for paste and shows why.
- Paste: ⌘V anywhere in the main window with a URL on the clipboard adds to the current bucket (or Inbox).
- Tray icon via Tauri tray API; closing the window hides it; "Quit" is in the tray menu. Notifications via `tauri-plugin-notification`; click payload is `{target_type, target_id}` and the frontend navigates there.
- Settings: hotkey, default "close tab", data dir path (read-only display), API keys (write-only fields backed by Keychain), AI provider/model, Whisper model choice, "Import all Chrome tabs…" button → confirm dialog with tab count and a "close them after import" checkbox.
- Chrome extension (slice 6): the app runs an `axum` listener on 127.0.0.1, preferring the previously saved port (default 47823, random only if taken) so the extension keeps working across restarts, with a token shown in Settings; MV3 extension with toolbar button and context menu POSTs `{url,title}`. Design only until slice 6.

### Frontend

Svelte 5 runes, Vite, TypeScript. State in a few `.svelte.ts` store modules wrapping `invoke()` calls, one per domain (`buckets`, `items`, `tasks`, `reminders`, `settings`). Notes editor is a `<textarea>` with a Markdown preview toggle (`marked` + `DOMPurify`) in slice 1; CodeMirror can come later. No component library; a small hand-rolled stylesheet with CSS variables, light/dark via `prefers-color-scheme`.

### Background jobs and errors

Snapshot, transcription, and AI are jobs with `pending/running/done/failed` status on their row and a retry action in the UI. Errors are stored in the `*_error` column and shown inline. Network is never on the capture path: save is synchronous to SQLite, metadata fetch and snapshot are jobs. The scheduler loop (slice 3) ticks every 30 s inside the Tauri process, marks `fired_at` before firing so a crash can't double-fire.

### Testing strategy

- `stow-core`: unit tests against a temp data dir (`tempfile`), covering repo CRUD, URL normalization, kind classification, FTS sync, metadata parsing from fixture HTML, due-reminder queries.
- `stow-cli`: integration tests with `assert_cmd` against a temp data dir.
- `stow-app`: `cargo check` on the Rust shell; Vitest for store modules with `@tauri-apps/api` mocked; manual run through `cargo tauri dev` for hotkey, Chrome, notifications.

### Build order (each slice is its own plan file)

1. Skeleton: workspace, db + migrations, buckets/items/notes/tasks repos, URL normalize + metadata fetch, CLI (`add`, `ls`, `buckets`, `mv`), Tauri shell with three-pane UI, paste capture, hotkey capture from Chrome with bucket picker and close-tab.
2. Snapshots (readability → md/html), attachments, FTS search UI.
3. Reminders, notifications, tray/background, Today view.
4. AI BYOK: Keychain, provider trait, Anthropic + OpenAI, five actions, AI tab.
5. Audio: cpal recording, whisper-rs transcription, model download, Recordings tab.
6. Import-all-tabs with confirm, Chrome extension + localhost endpoint.


