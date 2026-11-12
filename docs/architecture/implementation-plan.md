> Historical record of the initial build (2026-11-08). Superseded by the code, [docs/architecture/README.md](README.md) and the ADRs; kept for context.

# Stow Implementation Plan


**Goal:** Build Stow end to end: a local-first macOS Tauri app that parks Chrome tabs into intent buckets with notes, tasks, reminders, snapshots, attachments, audio transcripts, and BYOK AI.

**Architecture:** A Cargo workspace. `stow-core` holds all storage and logic against SQLite plus a data folder and has no UI dependency. `stow-cli` is a clap binary over core. `stow-app` is Tauri 2 with a Svelte 5 frontend and owns the window, tray, global hotkey, notifications, microphone, AppleScript to Chrome, and a localhost endpoint for the Chrome extension.

**Tech Stack:** Rust stable, rusqlite (bundled, FTS5), tokio, reqwest (rustls), scraper, dom_smoothie, htmd, cpal, hound, whisper-rs, keyring, axum, clap; Tauri 2 with global-shortcut, notification, clipboard-manager, dialog, opener plugins; Svelte 5 runes, Vite, TypeScript, marked, DOMPurify, Vitest.

**Spec:** [design-spec.md](design-spec.md) (the original spec, kept alongside this plan)

## Global Constraints

- Rust edition 2021, workspace at `Cargo.toml` at the repo root with members `crates/stow-core`, `crates/stow-cli`, `apps/stow-app/src-tauri`.
- Data dir default `~/Library/Application Support/Stow` (via `dirs::data_dir().join("Stow")`), overridable by `STOW_DATA_DIR` in the CLI and tests. Core always takes the dir as a parameter.
- IDs are ULID strings. Timestamps are RFC 3339 UTC strings (`chrono::Utc::now().to_rfc3339()`).
- API keys only in macOS Keychain, service `stow`, accounts `anthropic` and `openai`. Never in SQLite, logs, or events.
- Default AI models: Anthropic `claude-sonnet-5-5`, OpenAI `gpt-5-mini` (free-text setting, editable).
- Default Whisper model `base.en` from `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin`.
- Default hotkey `CmdOrCtrl+Shift+S`. Windows are labelled `main` and `capture`.
- Network is never on the capture path: `add_item` writes SQLite synchronously and enqueues jobs.
- Frontend: Svelte 5 runes, plain Vite (no SvelteKit), hash routing (`#/capture` renders the capture panel).

## Review Focus

1. Pasted or captured text that is not a plain http(s) URL (`chrome://extensions`, `javascript:`, a URL with surrounding whitespace or trailing sentence). Expected: trim and accept http(s); reject everything else with `StowError::InvalidUrl`. Test in Task 4.
2. Chrome not running, or Automation permission denied. Expected: never launch Chrome as a side effect; return a typed error the panel can explain. Test in Task 15 (osascript error mapping, System Events guard).
3. Importing a URL that already exists (dump-all after earlier saves). Expected: no duplicate row; the existing item moves into the requested bucket only if it was in the Inbox. Test in Task 3.
4. Reminder fired, snoozed, or app restarted between tick and notification. Expected: `due()` excludes fired reminders, respects `snoozed_until`, and `mark_fired` happens before the notification call so a crash cannot double-fire. Test in Task 9.
5. Metadata fetch fails (offline, 500, PDF). Expected: item stays saved, title falls back to the host name, status stays usable, snapshot marked `failed` with the error text. Test in Task 6 and Task 7.

---

## File structure

```
Cargo.toml                               workspace + [workspace.dependencies]
.gitignore
crates/stow-core/
  Cargo.toml
  migrations/0001_init.sql
  src/lib.rs                             pub mod exports, default_data_dir()
  src/error.rs                           StowError, Result
  src/db.rs                              Db::open, migrate, with()
  src/models.rs                          all structs/enums (serde)
  src/ids.rs                             new_id(), now()
  src/repo/mod.rs
  src/repo/{buckets,items,tasks,reminders,attachments,recordings,ai_runs,settings,search}.rs
  src/capture.rs                         normalize_url, classify_kind
  src/fetch/mod.rs                       http client factory
  src/fetch/metadata.rs                  Metadata, parse_metadata, parse_oembed, fetch_metadata, download_asset
  src/fetch/snapshot.rs                  Snapshot, extract, fetch_snapshot, write_snapshot
  src/jobs.rs                            Job, JobEvent, JobQueue, handlers
  src/ai/mod.rs                          Provider trait, provider_from_settings, run
  src/ai/{anthropic,openai,keys,prompts}.rs
  src/audio.rs                           RecorderHandle (thread + cpal), resample_linear, write_wav
  src/transcribe.rs                      Transcript, transcribe_wav, ensure_model
  tests/fixtures/{article.html,bare_title.html,youtube_oembed.json,readable.html}
crates/stow-cli/
  Cargo.toml
  src/main.rs                            clap commands
  tests/cli.rs                           assert_cmd
apps/stow-app/
  package.json, vite.config.ts, tsconfig.json, svelte.config.js, index.html, vitest.config.ts
  src/main.ts, src/App.svelte, src/app.css
  src/lib/types.ts, src/lib/api.ts, src/lib/url.ts
  src/lib/stores/{ui,buckets,items,tasks,reminders,settings}.svelte.ts
  src/lib/components/*.svelte            Sidebar, BucketHeader, ItemList, ItemRow, ItemDetail, NotesEditor,
                                         TaskList, ReminderList, AttachmentList, RecordingList, AiPanel,
                                         SearchBar, TodayView, SettingsView, CapturePanel, BucketPicker, Toasts
  src/lib/url.test.ts
  src-tauri/Cargo.toml, tauri.conf.json, build.rs, Info.plist, capabilities/default.json, icons/
  src-tauri/src/main.rs, lib.rs, state.rs, error.rs
  src-tauri/src/commands/{mod,buckets,items,tasks,reminders,attachments,recordings,ai,settings,search,chrome,window}.rs
  src-tauri/src/{chrome,scheduler,tray,shortcut,server}.rs
extension/manifest.json, background.js, options.html, options.js, icons/
```

---

### Task 1: Workspace, toolchain, core crate skeleton

**Files:** Create `Cargo.toml`, `.gitignore`, `crates/stow-core/Cargo.toml`, `crates/stow-core/src/{lib.rs,error.rs,ids.rs}`.

**Interfaces produced:**
- `stow_core::Result<T> = std::result::Result<T, StowError>`
- `StowError` variants: `Db(rusqlite::Error)`, `Io(std::io::Error)`, `NotFound { kind: &'static str, id: String }`, `InvalidUrl(String)`, `Http(reqwest::Error)`, `Json(serde_json::Error)`, `Keychain(String)`, `Audio(String)`, `Transcribe(String)`, `Ai(String)`, `Other(String)`; `#[derive(thiserror::Error, Debug)]`, `From` for the wrapped types.
- `ids::new_id() -> String` (ULID), `ids::now() -> String` (RFC 3339 UTC).
- `stow_core::default_data_dir() -> PathBuf`.

- [ ] Wait for rustup + cmake installs; `source ~/.cargo/env`; verify `cargo --version`, `cmake --version`.
- [ ] Write workspace `Cargo.toml` with `[workspace] members = ["crates/stow-core","crates/stow-cli","apps/stow-app/src-tauri"] resolver = "2"` and `[workspace.dependencies]` for: rusqlite (features bundled), serde (derive), serde_json, thiserror, tokio (full), reqwest (json, rustls-tls, no default), url, chrono, ulid, dirs, tempfile, anyhow, clap (derive), scraper, dom_smoothie, htmd, mime_guess, async-trait, keyring, cpal, hound, whisper-rs, axum, rand, sha2, tracing, tracing-subscriber, wiremock, assert_cmd, predicates.
- [ ] Core `Cargo.toml` depends on the subset it needs via `workspace = true`.
- [ ] Test `ids::tests::new_id_is_26_chars_and_sorts` and `now_parses_as_rfc3339`.
- [ ] `cargo test -p stow-core` passes.

### Task 2: Database open and migration

**Files:** Create `crates/stow-core/migrations/0001_init.sql`, `crates/stow-core/src/db.rs`, `src/models.rs`.

**Interfaces produced:**
- `Db::open(data_dir: &Path) -> Result<Db>`; creates dir and `snapshots/ attachments/ audio/ models/`; opens `stow.db`; `PRAGMA journal_mode=WAL; foreign_keys=ON`; applies migrations tracked in `schema_migrations(version INT pk)`.
- `Db::data_dir(&self) -> &Path`; `Db::with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T>`.
- `models.rs`: `Bucket, Item, ItemKind{Video,Article,Other}, JobStatus{None,Pending,Running,Done,Failed}, ParentType{Bucket,Item}, TargetType{Bucket,Item,Task}, Task, Reminder, Attachment, Recording, AiAction{SummarizePage,SummarizeNotes,SummarizeRecording,SuggestTasks,ExtractMetadata}, AiRun, SearchHit{doc_type,doc_id,item_id:Option,bucket_id:Option,title,snippet}` exactly as the spec's columns, all `#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)]`, enums `#[serde(rename_all="snake_case")]` with `as_str()`/`FromStr`.
- `0001_init.sql`: the spec's tables with CHECK constraints, `ON DELETE SET NULL` for `items.bucket_id`, indexes on `items(bucket_id, position)`, `tasks(parent_type,parent_id)`, `reminders(fire_at)`, and `CREATE VIRTUAL TABLE search USING fts5(doc_type UNINDEXED, doc_id UNINDEXED, item_id UNINDEXED, bucket_id UNINDEXED, title, body, tokenize='porter unicode61')`.

- [ ] Tests `db::tests::open_creates_dirs_and_tables` (asserts the 9 tables + `search` exist via `sqlite_master`) and `open_twice_is_idempotent`.
- [ ] Implement; `cargo test -p stow-core` passes.

### Task 3: Repos: buckets, items, tasks, settings, search

**Files:** Create `src/repo/mod.rs`, `src/repo/{buckets,items,tasks,settings,search}.rs`.

**Interfaces produced:**
- `buckets::create(db, name: &str, emoji: Option<&str>) -> Result<Bucket>` (position = max+1), `list(db) -> Result<Vec<Bucket>>` (non-archived, by position), `get(db, id)`, `update(db, id, patch: &BucketPatch{name,emoji,description_md,notes_md: Option<_>}) -> Result<Bucket>`, `archive`, `delete` (items go to Inbox), `reorder(db, ids: &[String])`.
- `items::create(db, new: NewItem{url: String, bucket_id: Option<String>, title: Option<String>}) -> Result<Item>`: normalizes URL (Task 4), classifies kind, dedupes by `canonical_url` (existing item returned; moved into `bucket_id` only if its `bucket_id` was None), title defaults to the given title or the host. `list(db, filter: ItemFilter{Inbox, Bucket(String), All}) -> Vec<Item>` ordered by `created_at DESC`, `get`, `move_to(db, id, bucket_id: Option<&str>)`, `set_notes`, `set_metadata(db, id, &Metadata, favicon_path, image_path)` (only fills fields that are Some, never overwrites a user-provided title with None), `set_snapshot_status(db, id, JobStatus, error: Option<&str>)`, `set_snapshot_done(db, id, fetched_at)`, `delete`, `find_by_canonical(db, url)`.
- `tasks::create(db, parent_type, parent_id, title) -> Task`, `list(db, parent_type, parent_id)` (by position), `toggle(db, id)`, `rename`, `delete`, `reorder`.
- `settings::get(db, key) -> Result<Option<String>>`, `set(db, key, value)`.
- `search::index(db, doc_type: &str, doc_id: &str, item_id: Option<&str>, bucket_id: Option<&str>, title: &str, body: &str)` (delete then insert), `remove(db, doc_type, doc_id)`, `query(db, q, limit) -> Vec<SearchHit>` using `snippet(search, 5, '[', ']', '…', 12)` and `bm25`. Doc types: `bucket`, `item`, `snapshot`, `recording`, `ai_run`. Buckets/items re-index on every write that changes name/title/description/notes.

- [ ] Tests (each its own `#[test]` on a `tempfile::tempdir()` db): `buckets_create_list_order`, `bucket_delete_moves_items_to_inbox`, `items_create_dedupes_by_canonical_and_moves_from_inbox`, `items_create_does_not_move_from_other_bucket`, `items_list_inbox_vs_bucket`, `items_title_defaults_to_host`, `tasks_toggle_sets_and_clears_done_at`, `settings_roundtrip`, `search_finds_item_by_notes_and_returns_item_id`.
- [ ] Implement; tests pass.

### Task 4: URL normalization and kind classification

**Files:** Create `src/capture.rs`.

**Interfaces produced:** `normalize_url(raw: &str) -> Result<Url>`, `classify_kind(url: &Url) -> ItemKind`, `extract_first_url(text: &str) -> Option<String>` (first http(s) token in pasted text).

Rules: trim; scheme must be http/https else `InvalidUrl`; lowercase scheme and host; drop fragment; drop query keys `utm_*`, `fbclid`, `gclid`, `dclid`, `msclkid`, `mc_cid`, `mc_eid`, `igshid`, `ref`, `ref_src`, `si`, `feature`; `youtu.be/<id>` and `youtube.com/shorts/<id>` and `m.youtube.com` → `https://www.youtube.com/watch?v=<id>` keeping only `v` and `t`; empty path → `/`. Video hosts: `youtube.com`, `youtu.be`, `vimeo.com`, `loom.com`, `twitch.tv` (with/without `www.`).

- [ ] Table-driven tests `normalize_cases` (≥10 rows incl. whitespace, utm strip, youtu.be, shorts, mobile, `chrome://x` → Err, `javascript:alert(1)` → Err, fragment drop), `classify_cases`, `extract_first_url_from_sentence`.
- [ ] Implement; tests pass.

### Task 5: Metadata fetch

**Files:** Create `src/fetch/mod.rs`, `src/fetch/metadata.rs`, fixtures `tests/fixtures/article.html`, `bare_title.html`, `youtube_oembed.json`.

**Interfaces produced:**
- `fetch::client() -> reqwest::Client` (desktop Safari-like UA, 15 s timeout, redirects ≤ 10).
- `Metadata { title, description, site_name: Option<String>, favicon_url, image_url: Option<Url> }`.
- `parse_metadata(html: &str, base: &Url) -> Metadata`: title from `og:title` → `<title>`; description from `og:description` → `meta[name=description]`; site_name from `og:site_name` → host; favicon from `link[rel~=icon]` resolved against base else `/favicon.ico`; image from `og:image`/`twitter:image`.
- `parse_oembed(json: &str, base: &Url) -> Metadata` (title, author_name → site_name "YouTube · {author}", thumbnail_url → image).
- `async fetch_metadata(client, url: &Url) -> Result<Metadata>`: YouTube hosts use `https://www.youtube.com/oembed?url={url}&format=json`; otherwise GET and parse only if content-type contains `text/html`, else `Metadata` with only `site_name = host`.
- `async download_asset(client, url: &Url, dest_dir: &Path, stem: &str) -> Result<PathBuf>` (extension from content-type, max 5 MB).

- [ ] Tests: `parse_article_prefers_og_tags`, `parse_bare_title_falls_back_to_host_and_favicon_ico`, `parse_oembed_youtube`, and with `wiremock`: `fetch_metadata_non_html_returns_site_only`, `fetch_metadata_500_is_error`.
- [ ] Implement; tests pass.

### Task 6: Snapshot extraction

**Files:** Create `src/fetch/snapshot.rs`, fixture `tests/fixtures/readable.html`.

**Interfaces produced:** `Snapshot { title: Option<String>, html: String, markdown: String, text: String }`, `extract(html: &str, base: &Url) -> Result<Snapshot>` (dom_smoothie readability → cleaned article HTML; htmd → Markdown; text = Markdown with syntax stripped for FTS), `async fetch_snapshot(client, url) -> Result<Snapshot>` (non-HTML content-type → `StowError::Other("not an html page")`), `write_snapshot(data_dir, item_id, &Snapshot) -> Result<(PathBuf, PathBuf)>` writing `snapshots/<id>.html` and `.md`, `read_snapshot_md(data_dir, item_id) -> Result<Option<String>>`.

- [ ] Tests: `extract_readable_drops_nav_and_keeps_paragraphs` (fixture has `<nav>`, `<aside>`, 3 `<p>`), `extract_markdown_has_heading`, `write_and_read_roundtrip`.
- [ ] Implement; tests pass.

### Task 7: Job queue

**Files:** Create `src/jobs.rs`.

**Interfaces produced:**
- `enum Job { FetchMetadata{item_id}, FetchSnapshot{item_id}, Transcribe{recording_id}, Ai{run_id} }`
- `enum JobEvent { ItemUpdated(String), RecordingUpdated(String), AiRunUpdated(String) }`
- `type JobHook = Arc<dyn Fn(JobEvent) + Send + Sync>`
- `JobQueue::start(db: Arc<Db>, hook: JobHook) -> JobQueue` (spawns a tokio task on the current runtime; one `tokio::spawn` per job; Transcribe runs in `spawn_blocking`); `enqueue(&self, job: Job)`; `JobQueue::noop_hook()`.
- Public handlers, so the CLI can run them inline: `async run_fetch_metadata(db: &Db, client: &Client, item_id: &str) -> Result<()>` (fetch, download favicon/image into `data_dir/assets/<item_id>.*`, `items::set_metadata`, re-index), `async run_fetch_snapshot(db, client, item_id) -> Result<()>` (status running → done/failed with error; index `snapshot` doc with `snapshot.text`). Transcribe/Ai handlers are filled in Tasks 10 and 11 and are `todo!()`-free stubs returning `Ok(())` until then.

- [ ] Tests with wiremock: `fetch_metadata_job_sets_title_and_emits_event`, `fetch_metadata_job_failure_keeps_item_and_host_title`, `fetch_snapshot_job_failure_sets_failed_with_error`, `fetch_snapshot_job_success_indexes_text` (search finds a word from the body).
- [ ] Implement; tests pass.

### Task 8: CLI

**Files:** Create `crates/stow-cli/Cargo.toml`, `src/main.rs`, `tests/cli.rs`.

Commands (clap derive, binary `stow`): `add <url> [-b <bucket name>]` (creates bucket if missing; runs metadata + snapshot inline; prints `id  title  kind`), `ls [-b <bucket>] [--all]` (default Inbox), `buckets` / `buckets new <name>`, `mv <item id or unique prefix> <bucket name | inbox>`, `search <query>`. Data dir from `STOW_DATA_DIR` else `default_data_dir()`.

- [ ] `assert_cmd` tests with `STOW_DATA_DIR` set to a tempdir and a wiremock URL: `buckets_new_then_list`, `add_then_ls_shows_item`, `mv_to_inbox`, `add_rejects_chrome_url` (exit code 1, stderr contains "invalid url").
- [ ] Implement; `cargo test -p stow-cli` passes.

### Task 9: Reminders, attachments, recordings, ai_runs repos

**Files:** Create `src/repo/{reminders,attachments,recordings,ai_runs}.rs`.

**Interfaces produced:**
- `reminders::create(db, target_type, target_id, fire_at, message: Option<&str>) -> Reminder`, `list_for(db, target_type, target_id)`, `list_upcoming(db, limit)` (not fired, by effective time), `due(db, now: &str) -> Vec<Reminder>` where effective = `COALESCE(snoozed_until, fire_at)` and `fired_at IS NULL`, `mark_fired(db, id, now)`, `snooze(db, id, until)` (sets `snoozed_until`, clears `fired_at`), `between(db, start, end)` for Today, `delete`, `delete_for_target`.
- `attachments::add(db, parent_type, parent_id, src: &Path) -> Attachment` (copies to `attachments/<id>/<filename>`, mime via `mime_guess`), `list`, `delete` (removes dir).
- `recordings::create(db, parent_type, parent_id, audio_path, duration_ms)`, `list`, `set_status(db, id, JobStatus, error)`, `set_transcript(db, id, md)` (status Done + index `recording` doc), `delete` (removes file).
- `ai_runs::create(db, parent_type, parent_id, action, provider, model, input_hash) -> AiRun` (status Pending), `start`, `complete(db, id, output_md)` (index `ai_run`), `fail(db, id, err)`, `list`, `get`.

- [ ] Tests: `reminder_due_excludes_fired_and_future`, `reminder_snooze_defers_and_refires`, `reminder_mark_fired_then_due_is_empty`, `attachment_add_copies_file_and_delete_removes`, `recording_transcript_indexed_in_search`, `ai_run_lifecycle`.
- [ ] Implement; tests pass.

### Task 10: Audio recording and Whisper transcription

**Files:** Create `src/audio.rs`, `src/transcribe.rs`; fill `Job::Transcribe` handler in `jobs.rs`.

**Interfaces produced:**
- `audio::resample_linear(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32>`; `audio::to_mono(frames: &[f32], channels: u16) -> Vec<f32>`; `audio::write_wav_16k(path, samples: &[f32]) -> Result<u64 /*duration_ms*/>` (i16 mono 16 000 Hz via hound); `audio::read_wav_16k_f32(path) -> Result<Vec<f32>>`.
- `audio::RecorderHandle::spawn() -> RecorderHandle` (dedicated std thread owning the cpal stream; commands over `std::sync::mpsc`), `start(&self, path: PathBuf) -> Result<()>`, `stop(&self) -> Result<RecordingInfo{path, duration_ms}>`, `is_recording(&self) -> bool`. Captures at the device default config, buffers f32, on stop converts mono → 16 k → WAV.
- `transcribe::Segment{start_ms,end_ms,text}`, `Transcript{segments}`, `Transcript::to_markdown()` → one line per segment `**[mm:ss]** text`; `transcribe_wav(model: &Path, wav: &Path) -> Result<Transcript>` (whisper-rs, `FullParams` Greedy, language "en", `token_timestamps` off, segment timestamps on); `async ensure_model(data_dir, name: &str, on_progress: impl Fn(u64, Option<u64>)) -> Result<PathBuf>` downloads to `models/ggml-{name}.bin` via a `.part` file.
- Job handler `run_transcribe(db, recording_id)`: status Running → `ensure_model` (name from settings `whisper_model`, default `base.en`) → `transcribe_wav` in blocking → `set_transcript`, or `set_status(Failed, err)`.

- [ ] Tests: `resample_halves_length_48k_to_24k`, `to_mono_averages_channels`, `wav_roundtrip_and_duration`, `transcript_markdown_format`, and `#[ignore] transcribe_real_model` (runs only when `STOW_WHISPER_MODEL` points at a file).
- [ ] Implement; non-ignored tests pass; `cargo build -p stow-core` links whisper.cpp.

### Task 11: AI providers

**Files:** Create `src/ai/{mod,anthropic,openai,keys,prompts}.rs`; fill `Job::Ai` handler.

**Interfaces produced:**
- `#[async_trait] trait Provider: Send + Sync { async fn complete(&self, system: &str, user: &str) -> Result<String>; fn name(&self) -> &'static str; fn model(&self) -> &str; }`
- `AnthropicProvider::new(api_key, model, base_url: Option<String>)`: POST `{base}/v1/messages`, headers `x-api-key` and `anthropic-version` (per the Anthropic Messages API reference), body `{model, max_tokens: 2048, system, messages:[{role:"user",content}]}`, returns `content[0].text`. `OpenAiProvider::new(api_key, model, base_url)`: POST `{base}/v1/chat/completions` with Bearer auth, returns `choices[0].message.content`. Non-2xx → `StowError::Ai(status + body excerpt)`.
- `keys::set(account, key)`, `get(account) -> Result<Option<String>>`, `delete(account)` via `keyring::Entry::new("stow", account)`.
- `provider_from_settings(db) -> Result<Box<dyn Provider>>` reading `ai_provider` (`anthropic` default | `openai`), `ai_model` (defaults above), key from Keychain; missing key → `StowError::Ai("no api key for <provider>")`.
- `prompts::AiContext { title, url, bucket_name: Option<String>, snapshot_text, notes, transcript: Option<String> }`, `prompts::build(action, &AiContext) -> (String, String)`; `prompts::load_context(db, parent_type, parent_id, action) -> Result<AiContext>` (snapshot text read from the `.md` file, capped at 60 000 chars; recording = latest transcript).
- Job handler `run_ai(db, run_id)`: `ai_runs::start` → provider → `build` → `complete` → `ai_runs::complete`, else `fail`.

- [ ] Tests with wiremock: `anthropic_provider_parses_text`, `openai_provider_parses_content`, `provider_error_status_is_ai_error`, `prompt_summarize_page_includes_title_and_text`, `provider_from_settings_without_key_errors` (use a `MockKeys` via `cfg(test)` or point keyring at an in-memory store if available; otherwise test the mapping function `select_provider(name, model, key: Option<String>)`).
- [ ] Implement; tests pass.

### Task 12: Tauri app scaffold

**Files:** Create `apps/stow-app/*` from the Tauri 2 Svelte-TS template (`npm create tauri-app@latest stow-app -- --template svelte-ts --manager npm`), then move `src-tauri` into the workspace (`[workspace]` empty table in its Cargo.toml is removed; root workspace lists it). Add plugins `global-shortcut`, `notification`, `clipboard-manager`, `dialog`, `opener`. `tauri.conf.json`: identifier `io.github.chaosdeselva.stow`, product name `Stow`, windows `main` (1200×780, min 900×600, title "Stow") and `capture` (520×320, `visible:false`, `alwaysOnTop`, `decorations:false`, `resizable:false`, `skipTaskbar`, url `index.html#/capture`). `capabilities/default.json` windows `["main","capture"]` with the plugin permissions. `Info.plist` with `NSAppleEventsUsageDescription` and `NSMicrophoneUsageDescription`. `src-tauri/src/{main,lib,state,error}.rs`: `AppState { db: Arc<Db>, jobs: JobQueue, recorder: RecorderHandle, http: reqwest::Client }`; `error.rs` `CmdResult<T> = Result<T, String>` and `impl From<StowError>`.

- [ ] `npm install`; `cargo check -p stow-app`; `npm run tauri dev` opens the template window; `npm run build` passes.

### Task 13: Tauri commands

**Files:** Create `src-tauri/src/commands/{mod,buckets,items,tasks,reminders,attachments,recordings,ai,settings,search,window}.rs`.

Commands (all `#[tauri::command]`, `State<AppState>`, return `CmdResult<_>`), names are the frontend contract:
- buckets: `list_buckets`, `create_bucket(name, emoji)`, `update_bucket(id, patch)`, `delete_bucket(id)`, `reorder_buckets(ids)`.
- items: `list_items(filter)`, `get_item(id)`, `add_item(url, bucket_id, title)` → create + enqueue `FetchMetadata` and `FetchSnapshot`, `move_item(id, bucket_id)`, `update_item_notes(id, notes_md)`, `delete_item(id)`, `refetch_item(id)`, `read_snapshot(id) -> Option<String>`.
- tasks: `list_tasks(parent_type, parent_id)`, `add_task(..., title)`, `toggle_task(id)`, `rename_task(id, title)`, `delete_task(id)`.
- reminders: `list_reminders(target_type, target_id)`, `list_upcoming_reminders(limit)`, `today_reminders()` (local day bounds computed in frontend and passed as `start,end`), `add_reminder(target_type, target_id, fire_at, message)`, `snooze_reminder(id, until)`, `delete_reminder(id)`.
- attachments: `list_attachments`, `add_attachment(parent_type, parent_id, src_path)`, `delete_attachment(id)`.
- recordings: `list_recordings`, `start_recording()` (temp path in `audio/tmp-<ulid>.wav`), `stop_recording(parent_type, parent_id) -> Recording` (rename to `audio/<id>.wav`, enqueue Transcribe), `is_recording`, `delete_recording(id)`, `retranscribe(id)`.
- ai: `list_ai_runs`, `run_ai(parent_type, parent_id, action) -> AiRun`, `set_api_key(account, key)`, `has_api_key(account)`, `delete_api_key(account)`.
- settings: `get_settings() -> Settings { close_tab_after_capture: bool, hotkey: String, ai_provider, ai_model, whisper_model, data_dir, extension_port: u16, extension_token }`, `update_settings(patch)` (re-registers hotkey when changed).
- search: `search(q)`.
- window: `hide_capture()`, `show_main()`.

Job hook emits `item-updated {id}`, `recording-updated {id}`, `ai-run-updated {id}` via `AppHandle::emit`.

- [ ] `cargo check -p stow-app` clean; smoke test by calling `add_item` from the template page's devtools console via `window.__TAURI__.core.invoke`.

### Task 14: Svelte UI (main window)

**Files:** Create `src/lib/types.ts` (mirrors models), `src/lib/api.ts` (one typed function per command), `src/lib/url.ts` (`extractFirstUrl`), stores, components, `App.svelte` with hash router, `app.css` (CSS variables, light/dark).

Behavior to pin:
- Sidebar: Inbox, Today, bucket list (emoji + name, drop target for item drag), "+ New bucket", Settings. Search field at top filters to a results list of `SearchHit`s; clicking a hit selects its item/bucket.
- Item list: favicon, title, site, kind badge, snapshot status dot (grey none/pending, blue running, green done, red failed with tooltip error). Right-click / "…" menu: Move to…, Refetch, Open in browser (opener), Delete.
- Detail: header (title, url link, site, bucket), tabs Notes / Page / Tasks / Reminders / Files / Recordings / AI.
- NotesEditor: textarea + Preview toggle (`marked` + `DOMPurify`), autosave debounced 600 ms and flushed on unmount/selection change (Review Focus: no lost edits).
- Bucket view: BucketHeader with editable name/emoji, bucket notes, bucket tasks, bucket reminders, then its items.
- Paste: `window` `paste` listener; if clipboard text contains a URL and focus is not in an input/textarea → `add_item` into current bucket (Inbox otherwise) and toast "Saved to <bucket>".
- Listen to `item-updated`, `recording-updated`, `ai-run-updated` → refresh that row.
- Toasts store for errors returned by commands.

- [ ] Vitest: `url.test.ts` (`extractFirstUrl` on three inputs), `stores/items.test.ts` with `@tauri-apps/api/core` mocked (`addItem` inserts into list, `applyUpdated` replaces by id).
- [ ] `npm run check` (svelte-check) and `npm test` pass; manual: create bucket, paste URL, title appears within seconds, notes persist after relaunch.

### Task 15: Chrome capture, global hotkey, capture panel

**Files:** Create `src-tauri/src/chrome.rs`, `commands/chrome.rs`, `shortcut.rs`, `src/lib/components/{CapturePanel,BucketPicker}.svelte`.

**Interfaces produced:**
- `chrome::ChromeError { NotRunning, PermissionDenied, NoTab, Script(String) }` with `Display` strings `chrome_not_running`, `permission_denied`, `no_tab`, `script_error: …`.
- `chrome::is_running() -> Result<bool, ChromeError>` via System Events (never `tell application "Google Chrome"` before this passes).
- `chrome::front_tab() -> Result<ChromeTab{url,title,window_index,tab_index}, ChromeError>`, `close_front_tab()`, `all_tabs() -> Vec<ChromeTab>` (AppleScript loop emitting `w\tt\turl\ttitle` lines), `close_tabs(tabs: &[ChromeTab])` (descending index order).
- `chrome::map_osascript_error(stderr: &str) -> ChromeError`: `-1743` or "Not authorized" → PermissionDenied; `-1728` or "Can't get" → NoTab; `-600` → NotRunning; else Script.
- Commands: `capture_front_tab`, `close_front_tab`, `list_chrome_tabs`, `import_chrome_tabs(bucket_id: Option<String>, close: bool) -> ImportResult{imported, skipped}` (skipped = deduped or non-http).
- `shortcut::register(app, hotkey)` → on press `open_capture(app)`: emit `capture-open` to window `capture`, show, center, set_focus. `shortcut::replace(app, old, new)`.
- CapturePanel: on `capture-open` calls `capture_front_tab`; shows URL + title (editable), BucketPicker (type-ahead over buckets, "New bucket “x”" row, Inbox), "Close tab in Chrome" checkbox bound to settings; Enter → `add_item` then `close_front_tab` if checked → `hide_capture`; Esc → `hide_capture`; error codes map to copy: not running → "Chrome isn't running. Paste a URL instead."; permission → "Stow needs Automation access to Chrome. System Settings → Privacy & Security → Automation."; no tab → "No Chrome tab is open."

- [ ] Rust unit tests: `map_osascript_error_cases`, `parse_all_tabs_output`. Manual: ⌘⇧S with Chrome frontmost shows the tab; Enter saves and closes tab; with Chrome quit shows the not-running copy.

### Task 16: Reminders UI, scheduler, notifications, tray

**Files:** Create `src-tauri/src/{scheduler,tray}.rs`, `src/lib/components/{ReminderList,ReminderForm,TodayView}.svelte`.

- `scheduler::start(app: AppHandle, db: Arc<Db>)`: tokio interval 30 s; for each `reminders::due(now)`: `mark_fired` first, then `tauri_plugin_notification` `.title(target title).body(message or "Reminder")`, then emit `reminder-fired {target_type, target_id}`. On startup run one tick immediately.
- Frontend: on `reminder-fired` store `lastFired`; on window focus, if `lastFired` is < 60 s old navigate to it (Tauri 2 notification clicks are not delivered on macOS, so this is the click behaviour). Today view lists `between(startOfDay, endOfDay)` plus overdue (`fire_at < now && fired_at IS NULL`) and upcoming 7 days, each with Snooze 1 h / Tomorrow 9am / Delete.
- ReminderForm: datetime-local input + message; quick buttons "In 1 hour", "Tonight 7pm", "Tomorrow 9am".
- `tray.rs`: tray icon with menu Open Stow / Quick Capture / Quit; main window `CloseRequested` → `prevent_close` + `hide`; tray click shows main.

- [ ] Rust test `scheduler::tests::due_reminder_is_marked_before_notify` (inject a notify closure that panics; assert `fired_at` set). Manual: add a reminder 1 min out, close the window, notification appears, clicking it focuses Stow on the target.

### Task 17: Snapshot reader, attachments UI, search UI

**Files:** Create `src/lib/components/{PageView,AttachmentList}.svelte`; wire SearchBar results.

- PageView: renders `read_snapshot` Markdown via marked+DOMPurify, shows status/error with Retry (`refetch_item`), "Open original".
- AttachmentList: "Add file…" (dialog plugin `open`, multiple) → `add_attachment` per path; rows with filename, size, Open (opener `openPath`), Delete. Drag-and-drop files onto the detail pane uses Tauri `tauri://drag-drop` event paths.
- Search results list shows doc type chips.

- [ ] Manual: snapshot of a news article reads cleanly; attach an `.md`, open it, delete it and the folder is gone; search a word from the snapshot and from an attachment name.

### Task 18: Recordings UI and AI panel

**Files:** Create `src/lib/components/{RecordingList,AiPanel}.svelte`, API key fields in `SettingsView`.

- RecordingList: Record/Stop button with elapsed timer; rows with duration, status, transcript (collapsible), Retranscribe, Delete, "Insert transcript into notes". First recording triggers the mic permission prompt; first transcription shows model download progress via `model-download-progress {received,total}` event (emit from `ensure_model` progress callback through the job hook: add `JobEvent::Progress{kind,received,total}`).
- AiPanel: provider/model shown; buttons per action available for this parent (SummarizePage only when snapshot done; SummarizeRecording only when a transcript exists; SuggestTasks adds a "Add these as tasks" button that parses `- ` lines); run list with status, output rendered, Copy, "Append to notes".
- SettingsView: provider select, model text, API key inputs (write-only, show "Key saved" when `has_api_key`), Whisper model select (`tiny.en`, `base.en`, `small.en`), hotkey text with Apply, close-tab default, data dir, extension port/token with Copy, "Import all Chrome tabs…" (Task 19).

- [ ] Manual: record 10 s, transcript appears; summarize page with a real key; suggest tasks and add them.

### Task 19: Import all Chrome tabs and the Chrome extension

**Files:** Create `src-tauri/src/server.rs`, `extension/{manifest.json,background.js,options.html,options.js}`, Settings import dialog.

- Import dialog: calls `list_chrome_tabs`, shows count and the list (checkboxes, all on), bucket picker (default new bucket "Tabs <date>"), "Close imported tabs" checkbox, Confirm → `import_chrome_tabs`, result toast "Imported N, skipped M".
- `server::start(app, db, jobs) -> u16`: axum on `127.0.0.1:0`; token from settings `extension_token` (generate 32 hex chars once); routes `GET /health` → `{"app":"stow","version"}`; `GET /buckets`, `POST /capture {url,title,bucket_id?}` both require `Authorization: Bearer <token>`; CORS allows `chrome-extension://*`. Port saved to settings `extension_port` each start.
- Extension (MV3): toolbar click and context menu "Stow this page" → POST `/capture` with stored port/token/default bucket; badge ✓ on success, ✕ on failure; optional close tab. Options page stores port, token, default bucket (fetched from `/buckets`), close-tab toggle.

- [ ] Rust tests with `axum::test` or `reqwest` against a bound server: `capture_requires_token`, `capture_adds_item`. Manual: load unpacked extension, set port/token, click toolbar → item appears.

### Task 20: Verification pass

- [ ] `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `npm run check`, `npm test`, `npm run tauri build` produces `Stow.app`.
- [ ] Walk the spec's verification list (CLI add/ls, paste, hotkey capture, close tab, denied permission copy, reminder fires, recording transcribes, AI summarizes, import dialog, extension capture).
- [ ] Write `README.md` (what it is, build, data location, extension install) and update the spec file if anything diverged.
