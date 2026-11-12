# Stow Tauri command/event contract

Binding for `apps/stow-app/src` (TypeScript) and `apps/stow-app/src-tauri` (Rust).

## Conventions
- Rust command names are snake_case; invoke them by that exact name: `invoke('list_items', {...})`.
- **Argument keys are camelCase on the JS side** (Tauri default conversion): Rust `bucket_id` → JS `bucketId`, `parent_type` → `parentType`, `notes_md` → `notesMd`, `src_path` → `srcPath`, `fire_at` → `fireAt`.
- **Returned structs keep snake_case field names** exactly as in `crates/stow-core/src/models.rs` (e.g. `bucket_id`, `notes_md`, `snapshot_status`, `created_at`).
- Enums serialize as snake_case strings: `kind: 'video'|'article'|'other'`, status `'none'|'pending'|'running'|'done'|'failed'`, `parent_type: 'bucket'|'item'`, `target_type: 'bucket'|'item'|'task'`, `action: 'summarize_page'|'summarize_notes'|'summarize_recording'|'suggest_tasks'|'extract_metadata'`.
- Errors: every command rejects with a plain string. Chrome commands reject with one of `chrome_not_running`, `permission_denied`, `no_tab`, or `script_error: <detail>`.
- Timestamps are RFC 3339 UTC strings. The frontend converts to/from local time.

## Types (TS mirrors of models.rs)
Bucket, Item, Task, Reminder, Attachment, Recording, AiRun, SearchHit — same fields as models.rs.
```ts
type ItemFilter = { kind: 'inbox' } | { kind: 'bucket'; id: string } | { kind: 'all' };
type BucketPatch = { name?: string; emoji?: string | null; description_md?: string; notes_md?: string }; // omit = keep, emoji null = clear
type Settings = { theme: 'light'|'dark'|'system' /* default light */; close_tab_after_capture: boolean; hotkey: string; ai_provider: 'anthropic'|'openai'; ai_model: string; whisper_model: 'tiny.en'|'base.en'|'small.en'; data_dir: string; extension_port: number; extension_token: string };
type ChromeTab = { window_index: number; tab_index: number; url: string; title: string };
type ImportResult = { imported: number; skipped: number };
type ReminderAgenda = { overdue: Reminder[]; today: Reminder[]; upcoming: Reminder[] };
```

## Commands (name(jsArgs) → return)
App: `app_info()` → `{name, version, data_dir}`
Buckets: `list_buckets()` → Bucket[]; `create_bucket({name, emoji?})` → Bucket; `update_bucket({id, patch: BucketPatch})` → Bucket; `delete_bucket({id})`; `reorder_buckets({ids})`
Items: `list_items({filter: ItemFilter})` → Item[]; `get_item({id})` → Item; `add_item({url, bucketId?, title?})` → Item (also enqueues metadata + snapshot jobs); `move_item({id, bucketId: string|null})` → Item; `update_item_notes({id, notesMd})` → Item; `delete_item({id})`; `refetch_item({id})`; `read_snapshot({id})` → string|null (Markdown)
Tasks: `list_tasks({parentType, parentId})` → Task[]; `add_task({parentType, parentId, title})` → Task; `toggle_task({id})` → Task; `rename_task({id, title})` → Task; `delete_task({id})`
Reminders: `list_reminders({targetType, targetId})` → Reminder[]; `add_reminder({targetType, targetId, fireAt, message?})` → Reminder; `snooze_reminder({id, until})` → Reminder; `delete_reminder({id})`; `reminder_agenda({start, end})` → ReminderAgenda (start/end = local day bounds as RFC3339 UTC; overdue = unfired before start, today = effective time in [start,end), upcoming = unfired after end, max 50)
Attachments: `list_attachments({parentType, parentId})` → Attachment[]; `add_attachment({parentType, parentId, srcPath})` → Attachment; `delete_attachment({id})`
Recordings: `list_recordings({parentType, parentId})` → Recording[]; `is_recording()` → boolean; `start_recording({parentType, parentId})` (parent is remembered; it wins over the one passed to stop); `stop_recording({parentType, parentId})` → Recording (enqueues transcription); `delete_recording({id})`; `retranscribe({id})`
AI: `list_ai_runs({parentType, parentId})` → AiRun[]; `run_ai({parentType, parentId, action})` → AiRun (pending; result arrives via event); `set_api_key({account, key})`; `has_api_key({account})` → boolean; `delete_api_key({account})` (account = 'anthropic' | 'openai')
Settings: `get_settings()` → Settings; `update_settings({patch: Partial<Settings>})` → Settings (hotkey change re-registers the shortcut)
Search: `search({q})` → SearchHit[] (max 50)
Chrome: `capture_front_tab()` → ChromeTab; `close_front_tab({expectedUrl?})` (closes the front tab only if it still shows expectedUrl); `list_chrome_tabs()` → ChromeTab[]; `import_chrome_tabs({tabs: ChromeTab[], bucketId: string|null, close: boolean})` → ImportResult
Window: `hide_capture()`; `show_main({targetType?, targetId?})` (shows/focuses main and emits `navigate` when a target is given)

Opening things: use `@tauri-apps/plugin-opener` `openUrl(url)` for the original page and `openPath(path)` for attachments/snapshots. File picking: `@tauri-apps/plugin-dialog` `open({multiple: true})`.

## Events (listen with `@tauri-apps/api/event`)
- `buckets-changed` `{}` — buckets created/renamed/deleted/reordered from any window; reload the bucket list
- `item-added` `{id}` — a new item was inserted by any path (panel, paste, CLI-free extension, import); fetch it and insert if it matches the current filter, bump Inbox count
- `item-updated` `{id}` — metadata or snapshot landed; refetch that item
- `app-will-quit` `{}` — flush pending autosaves immediately; the app exits ~800 ms later
- `recording-updated` `{id}` — transcript landed/failed
- `ai-run-updated` `{id}` — run finished/failed
- `model-download-progress` `{received, total: number|null}`
- `reminder-fired` `{reminder_id, target_type, target_id, title}`
- `navigate` `{target_type, target_id}` — select this bucket/item/task's parent
- `capture-open` `{}` — sent to the `capture` window when the hotkey opens it; the panel should re-run `capture_front_tab`

## v0.1 hardening additions (2026-11-08, after the AAA review)

### Errors become structured
Every command now rejects with `CmdError = { code: string; message: string }` instead of a bare string. Codes: `not_found`, `invalid_url`, `invalid_input`, `db`, `io`, `http`, `not_html`, `too_large`, `keychain`, `audio`, `transcribe`, `provider` (AI; message starts with provider + status), `chrome_not_running`, `permission_denied`, `no_tab`, `script_error`, `hotkey`, `internal`. The frontend's `friendlyError(e)` maps codes (falling back to string parsing for legacy strings) to user copy and keeps `message` for a "Details" disclosure.

### Changed commands
- `add_item({url, bucketId?, title?})` → `{ item: Item, created: boolean }` (was Item).
- `close_front_tab({expectedUrl?})` → `{ closed: boolean }`.
- `list_items({filter, includeDone?: boolean})`; `Item` gains `done_at: string|null`. New `set_item_done({id, done})` → Item.
- `get_settings()` → Settings gains: `hotkey_registered: boolean`, `hotkey_error: string|null`, `onboarding_dismissed: boolean`, `onboarding_steps: Record<string, boolean>` (keys: captured, automation, notifications, extension, ai_key, whisper), `close_to_tray_explained: boolean`, `quit_on_close: boolean`, `open_at_login: boolean`, `ai_consent_given: boolean`. `update_settings` accepts all of them; `hotkey` default is now `Ctrl+Cmd+S` (shown as ⌃⌘S).
- `update_settings` validates `ai_provider ∈ {anthropic, openai}`, `whisper_model ∈ {tiny.en, base.en, small.en}`, theme; invalid → `invalid_input`.
- All ids are validated as ULIDs (26 Crockford base32 chars) at the command boundary → `invalid_input`.

### New commands
- `permission_status()` → `{ automation: 'granted'|'denied'|'not_determined'|'chrome_not_running', microphone: 'granted'|'denied'|'not_determined'|'restricted' }` (no prompts are triggered). Notifications are checked from the frontend via `@tauri-apps/plugin-notification` `isPermissionGranted`/`requestPermission`.
- `request_automation()` → same automation status after prompting (AEDeterminePermissionToAutomateTarget with askUserIfNeeded=true, target com.google.Chrome). `request_microphone()` → microphone status after prompting (AVCaptureDevice requestAccess).
- `open_privacy_settings({pane: 'automation'|'microphone'|'notifications'})`.
- `pairing_code()` → `"<port>:<token>"`; `rotate_extension_token()` → new pairing code (old token stops working immediately).
- Extension auto-pairing: the extension may `POST /pair {name}` with no token; the app emits `pair-request {id, name}`; the frontend shows Allow/Deny; `respond_pair({id, allow})` resolves it (the HTTP call waits up to 60 s and returns `{token, port}` on allow, 403 on deny/timeout).
- `export_library({format: 'markdown'|'json', destDir})` → `{ path, buckets, items }` (Markdown: one folder per bucket + `Inbox/`, one `.md` per item with front-matter url/saved/kind/done and sections Notes/Tasks/Transcripts/AI; JSON: full dump). `backup_database({destPath})` → `{ path, bytes }` (SQLite online backup).
- `ready_to_quit()` — the frontend calls it after flushing autosaves on `app-will-quit`; the app exits immediately (the 800 ms timer stays as a fallback).
- Autostart: frontend uses `@tauri-apps/plugin-autostart` (`enable/disable/isEnabled`); `open_at_login` in Settings mirrors it.
- `delete_item` is unchanged; undo is implemented in the frontend by deferring the call 6 s behind an "Undo" toast.

### New events
- `pair-request` `{id, name}` (see above) · `hotkey-status` `{hotkey, registered, error}` (after startup registration and after changes).

### Rendering rules (security)
- AI output and notes render with DOMPurify `FORBID_TAGS: ['img','form','input','button','iframe','object','embed','video','audio','svg','math']`; snapshot pages render remote images as links unless the per-item "Load images" toggle is on.
- CSP is set in tauri.conf.json; `withGlobalTauri` is false; the asset scope covers only `…/Stow/assets/**` and `…/Stow/audio/**` (recording playback); opener `open-path` only `…/Stow/attachments/**`.

## Tauri phase notes (2026-11-08)

Deviations from and additions to the sections above, as implemented in `apps/stow-app/src-tauri`:

- **Extra commands:** `get_task({id})` → Task; `whisper_model_downloaded({model})` → boolean; `download_whisper_model({model})` → `{path}` (progress via `model-download-progress` with `kind: 'whisper_model'`; unknown model → `invalid_input`).
- **`app_info()`** → `{name, version, data_dir, extension_dir: string|null}` (`extension_dir` is the bundled unpacked extension in `Stow.app/Contents/Resources/extension`, or the repo's `extension/` in dev builds).
- **`request_automation()`** returns the automation status string and **`request_microphone()`** returns the microphone status string (not the whole `permission_status` object).
- **Settings** has one more field, `extension_id: string` (empty = any `chrome-extension://` origin may call the local server; set it to the 32-letter id to pin CORS to that extension). `update_settings` accepts it (must be empty or 32 letters a–p).
- **`onboarding_steps` in `update_settings` is merged** into the stored object (only the keys sent change). Unknown step keys or non-boolean values → `invalid_input`.
- **Hotkey change that fails to register** rejects with code `hotkey`; nothing in that patch is saved, the old shortcut is re-registered, and `hotkey-status` is emitted. `hotkey_registered` is false when the stored hotkey is not the one currently registered.
- **New event `open-settings` `{}`**, sent to `main` when the tray's "Settings…" item is chosen; the frontend should open its Settings view.
- **Notification click fallback:** macOS doesn't tell us which notification was clicked. If the app is reopened (dock click / notification click) within 60 s of a reminder firing, Rust shows main and emits `navigate` for that reminder's target. The notification body is the reminder message, or the target's title when there is no message.
- **`open_privacy_settings({pane: 'notifications'})`** opens the Notifications pane (`com.apple.preference.notifications`); notifications have no Privacy & Security anchor.
- **`respond_pair`** for an unknown or expired id rejects with `not_found`.
- **Local server:** `/health` returns `{app: 'stow'}` (version dropped); `Host` must be `127.0.0.1:<port>` or `localhost:<port>` (else 400); bodies over 16 KiB → 413; url > 8 KiB or title > 1 KiB → 400; `/pair` while another request is waiting → 429.
- **`export_library` / `backup_database`**: see the status line at the end of this section.
- **Capabilities:** each window has its own. `capture` may call only `add_item, capture_front_tab, close_front_tab, hide_capture, list_buckets, create_bucket, get_settings, update_settings, app_info`, plus event listen, basic window calls and clipboard read/write. `main` may call every command. The JS global-shortcut permissions were removed: Rust owns the shortcut.
- **Asset/opener scopes** follow the hardening section. `openPath` also allows the data dir itself (for "Open in Finder"), and `revealItemInDir` is allowed (opener default). Snapshots are no longer openable with `openPath`; use `read_snapshot`.
- **Export status:** `export_library` and `backup_database` call `stow_core::export`. `format` other than `markdown`/`json`, or a relative `destDir`/`destPath`, rejects with `invalid_input`. The `ExportReport.path` is the folder (Markdown) or the file (JSON).
- **Quit:** on exit the job queue gets up to 5 s to finish running jobs (`JobQueue::shutdown`). Jobs it cuts off resume on next launch.

### Integrator notes (after phase 2)
- New error code `busy`: `refetch_item` / `retranscribe` reject with it when the same job is already queued or running (message is user-ready).
- Settings gains `allow_local_urls: boolean` (default false; restart required; shown under Settings → Data → Advanced with copy "Fetch pages on private or local addresses (e.g. http://localhost). Off by default for safety. Takes effect after restarting Stow."). Captured local URLs are always saved; with this off their readable copy is marked failed with "local address not fetched".

## Tauri polish notes (2026-11-08)

Shell-only changes in `apps/stow-app/src-tauri`; the frontend sees new commands, two new events and a few behavior changes.

### Capture panel (UX P1-4)
- Path taken: **plain `NSWindow` with Space behaviors, not an `NSPanel`.** `tauri-nspanel` 2.1.1 does build against tauri 2.12, but it hard-requires tauri's `macos-private-api` Cargo feature and swaps the window's class at runtime (`object_setClass`), and WKWebView text input inside a non-activating panel needs its own workarounds. Not worth it for a polish pass. Focus hand-back stays as before: `hide_capture` calls `app.hide()` when Stow was not the active app, so Chrome comes back to the front. Consequence: while the panel is open, Stow is the active app, so a visible main window is still brought forward with it (only a non-activating panel fixes that).
- `src/capture_panel.rs`: at setup the `capture` window gets `set_visible_on_all_workspaces(true)` plus `NSWindowCollectionBehaviorCanJoinAllSpaces | FullScreenAuxiliary` via objc2-app-kit, so it appears on the current Space, including a full-screen Chrome Space. `MoveToActiveSpace` is **not** combined with it: Apple documents it as mutually exclusive with `CanJoinAllSpaces`. The floating level comes from `alwaysOnTop: true` (tao sets `NSFloatingWindowLevel`).
- `tauri.conf.json`: the capture window is now `transparent: true` (+ `shadow: true`), without `macOSPrivateApi` (tauri 2.12 no longer needs it for a transparent window). The frontend's `#/capture` page must give `html`/`body` a transparent background and draw the card itself: 10 px radius, hairline border. Until it does, the page's own background shows as a square.

### Danger zone (Settings → Data)
- `delete_whisper_models()` → `{ deleted_bytes: number }`. Removes `<data_dir>/models/*.bin` and `*.part`. Permission `allow-delete-whisper-models` (main only).
- `reset_all_data({ confirm })` → `()` then the app restarts. `confirm` must be exactly `"delete"` (else `invalid_input`). Stops the job queue (`JobQueue::shutdown`, up to 5 s), deletes Stow's own files in the data dir (`stow.db` and its `-wal`/`-shm` sidecars, plus the `snapshots/`, `attachments/`, `audio/`, `models/`, `assets/` folders; the dir itself and anything else in it stay, which matters for a custom `STOW_DATA_DIR`), then `request_restart()`; the `app-will-quit` flush is skipped (the data is gone). The frontend should show a blocking "Resetting…" state; the window disappears when the process restarts. Permission `allow-reset-all-data` (main only).

### Opening files (custom data dir)
- `open_attachment({ id })` → `()`. Opens the attachment in its default app via the opener plugin's Rust API, after checking the stored path is inside `<data_dir>/attachments`. Use this instead of `openPath(attachment.path)`: the plugin's `opener:allow-open-path` scope is fixed at build time to the default data dir, so `openPath` fails under a custom `STOW_DATA_DIR`. Permission `allow-open-attachment` (main only).
- `reveal_data_dir()` → `()`. Opens the data folder in Finder (replaces `openPath(app_info.data_dir)` for the same reason). Permission `allow-reveal-data-dir` (main only). `revealItemInDir` is not scope-checked by the plugin and keeps working for any path.
- Startup: when the database cannot be opened (including a schema newer than the build), the app shows a native error dialog with the message and the data folder path, then exits with status 1; no window appears.
- Recordings stop themselves after 3 hours (`stow_core::audio::MAX_RECORDING`): the WAV is filed under the parent given to `start_recording`, `recording-state {recording: false}` and `recording-updated {id}` are emitted, and a notification "Recording stopped after 3 hours" is shown.

### Notification click (UX P0-4)
- `tauri-plugin-notification` 2.8 does report clicks on macOS (`on_action`, action `tap`). Reminder notifications now carry `extra: {target_type, target_id}` and a Rust `on_action` handler shows main + emits `navigate` for that target. No frontend change.
- Fallback kept: reopening the app (dock click / `RunEvent::Reopen`) within **120 s** (was 60 s) of a reminder firing navigates to it; `Reopen` with main hidden shows main (already did). A handled click clears the fallback so a later dock click does not repeat the navigation.

### Settings
- `hotkey` must parse **and** include at least one modifier (Cmd/Ctrl/Alt/Shift/Super); bare keys (`S`, `F5`, `Space`) → `invalid_input` before any registration attempt. The hotkey is still persisted only after registration succeeds.
- **New narrow commands for the capture window** (frontend: switch the panel to these):
  - `capture_settings()` → `{ close_tab_after_capture: boolean, hotkey: string, theme: string }` — no token, no other fields.
  - `set_close_tab_after_capture({ value: boolean })` → the same `CaptureSettings` object.
  - Permissions `allow-capture-settings`, `allow-set-close-tab-after-capture` are in **both** capabilities. The `capture` capability lost `allow-get-settings`, `allow-update-settings` and `clipboard-manager:allow-write-text`; `get_settings`/`update_settings` are main-only now.

### Tray
- Dynamic section between "Paste Link from Clipboard" and "Settings…": up to 3 `Due: <title>` items (reminders whose effective time fell in the last 24 h, newest first, titles cut at 40 chars) and, while recording, `Recording… ■ Stop`. Clicking a due item shows main and emits `navigate` for the reminder's target; Stop files the recording under the parent passed to `start_recording` (emits `recording-updated {id}`; with no parent the WAV is kept and a warning logged). The menu is rebuilt on `reminder-fired`, on recording start/stop (any path), after `snooze_reminder`/`delete_reminder`, and on hotkey change.
- "Paste Link from Clipboard" now takes the first `http(s)://` token in the clipboard text (`stow_core::capture::extract_first_url`), so a pasted sentence containing a link works.

### Recording
- **New event `recording-state` `{ recording: boolean }`**, emitted to every window when recording starts or stops by any path (command or tray). The UI should drive its record button from it instead of only from its own calls.
- `start_recording` while already recording → `busy` (was `audio`). `stop_recording` when the recorder thread died → `audio` ("recorder thread stopped"), never a panic.
- `stop_recording`: if the database insert fails after the WAV was moved, the `io` error names the kept path (`… (audio kept at <path>): <cause>`), same as the existing rename failure.
- Quitting while recording stops the recorder and saves the WAV as a recording for the remembered parent (or keeps the temp file under `audio/` and logs when there is none).

### Robustness / lifecycle
- `capture_front_tab` with no usable `pgrep` → `script_error: pgrep: <cause>`.
- Clean quit: after `JobQueue::shutdown`, `running` rows (`items.snapshot_status`, `recordings.transcript_status`, `ai_runs.status`) are set back to `pending`, so the next launch resumes them instead of failing them as "interrupted".
- `refetch_item` / `retranscribe` now write `pending` (error cleared, `*_attempts = 0`) **before** enqueueing; a duplicate in flight still returns `busy` and nothing is reverted (the running job overwrites the status when it finishes).
- App Nap is off: `NSAppSleepDisabled` in Info.plist plus a process-lifetime `NSProcessInfo beginActivityWithOptions:NSActivityBackground reason:"Stow reminders and background jobs"` token.
- `tauri-plugin-single-instance`: a second launch (LaunchAgent + manual, double-click while in the tray) focuses the running instance's main window and exits, so reminders never double-fire.
- `app_info()` gains `build: { version: string, commit: string|null, built_at: string }` (`built_at` RFC 3339 UTC from build time; `commit` from `STOW_COMMIT` at build time, null locally).
