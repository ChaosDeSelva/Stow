# Changelog

All notable changes to Stow are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Before 1.0, minor versions may include breaking changes; schema changes always ship as a new migration.

## [Unreleased]

The first release. "Changed" and "Fixed" describe differences from the pre-release builds that circulated before it.

### Added
- **Capture.** Press ⌃⌘S with Chrome in front to save the current tab into a bucket, create a bucket on the spot, and optionally close the tab (on by default). The shortcut is configurable in Settings → Capture; if another app owns it, Stow says so and keeps the previous one.
- **Add link** button (⌘L) above every list: paste one URL or many, separated by spaces, commas, or new lines, and pick the bucket. The form pre-fills links from the clipboard (**From clipboard**), counts what it found and says how many lines it ignored, takes an optional title for a single link, saves with ⏎ (**Save** / **Save N**), reports duplicates with **Show it** / **Show them** instead of saving them twice, and keeps any link that failed in the field with the reason.
- Paste-to-capture (⌘V in the main window), **Paste Link from Clipboard** in the menu-bar menu, and **Import all Chrome tabs…** (Settings → Capture) with a per-tab checklist and an optional close.
- **Buckets** with emoji, description, notes, tasks, and reordering; an **Inbox** for unsorted links; an **All items** view.
- **Archive.** Press A (or click Archive on a row or in the detail header) when you're done with a link. Archived links live in the **Archived** view, where **Restore** brings them back; **Show archived** reveals them inside a bucket. Archive and Delete both show an **Undo** toast for six seconds.
- **Multi-select.** ⌘-click, ⇧-click, or ⌘A select several links; a bar shows **N selected** with **Archive** / **Restore**, **Move to…**, **Delete**, and **Clear** for the whole batch, with one Undo. The A, M (move), and ⌫ keys act on the selection; Esc clears it.
- Duplicate detection by canonical URL: tracking parameters (`utm_*`, `fbclid`, `gclid`, …) and fragments are stripped and YouTube URLs are reduced to the video id. Saving a link you already have selects it (and moves it out of the Inbox if you chose a bucket).
- Background metadata fetch (title, description, site name, favicon, preview image; YouTube via oEmbed) and an offline readable copy of each article as Markdown and HTML. The **Page** tab shows the copy, renders remote images as links unless the per-item **Load images** toggle is on, and offers **Download page again** (**Try again** after a failure); **Open original** (**Watch** for videos) sits in the detail header.
- Markdown **notes** (autosaved, with preview) and **tasks** on every bucket and link.
- **Reminders** on buckets, links, and tasks with quick times (**+1h**, **Tonight**, **Tomorrow 9:00**, **Next week**, **This weekend**, **Next Monday**), native macOS notifications, snooze, and a **Today** view (⌘2) of **Overdue**, **Today**, and **Upcoming** reminders. Clicking a notification opens Stow on the reminder's link, bucket, or task; reopening Stow within two minutes of a notification does the same; the menu-bar menu lists up to three **Due: …** entries that navigate too.
- **Files**: attach any file to a bucket or link; Stow keeps its own copy.
- **Recordings** with on-device transcription (whisper.cpp with Metal on Apple Silicon). Models `tiny.en` (75 MB), `base.en` (142 MB, default), and `small.en` (466 MB) download on first use with a progress bar, or from Settings → Transcription → **Download now**. Transcripts have timestamps, are searchable, and can be re-run (**Transcribe again**) or appended to notes (**Insert into notes**).
- **AI actions** with your own Anthropic or OpenAI key: **Summarize page**, **Summarize notes**, **Summarize recording**, **Suggest tasks** (with a **Pick the tasks to add** preview and **Add N tasks**), and **Fix title & description** for pages whose title is just the host name. Each result has **Copy** and **Append to notes**. Keys are stored in the macOS Keychain; a one-time consent step (**I understand** in Settings, or the **Send to …?** dialog on the first action) explains what is sent.
- **Search** (⌘K or ⌘F) across titles, descriptions, notes, readable copies, transcripts, and AI output, ranked with titles weighted higher.
- **Menu-bar mode**: closing the window keeps Stow running so reminders fire; **Quit Stow** when the window closes and **Open Stow at login** are in Settings → General. Stow runs as a single instance (launching it again focuses the running copy) and opts out of App Nap so reminders fire on time in the background.
- **Get set up**: a six-step view (**Save your first tab**, **Let Stow see your Chrome tab**, **Turn on reminders**, **Add the Chrome button**, **Add an AI key**, **Voice notes**) shown as its own sidebar entry above the Inbox on first launch with an N/6 badge; steps tick themselves as you use the features; it disappears when done or dismissed and can be brought back with Settings → General → **Show setup checklist**.
- **Permissions** section in Settings showing Automation, Microphone, and Notifications status with **Ask now**, **Open Privacy Settings**, and **Test** buttons. Stow never prompts until you use the feature.
- **Chrome extension** (Manifest V3): toolbar button, **Stow this page** and **Stow this link** context menus, a default bucket, and an optional close-tab. Pair it with one click (**Pair automatically**, approved in Stow) or by pasting the `port:token` code from Settings → Chrome extension. The unpacked extension ships inside the app bundle.
- **Export** (Settings → Data or `stw export`) as Markdown folders (one folder per bucket, one `.md` per link with front matter and Notes / Tasks / Transcripts / AI notes / Files sections, attachments copied alongside) or one JSON file; **Back up database…** (`stw backup`) makes a consistent copy of `stow.db` while Stow runs.
- **`stw` CLI**: `add` (`--bucket`, `--title`, `--no-fetch`), `ls` (`--bucket`, `--all`, `--json`), `buckets` / `buckets new`, `mv`, `search` (`--rebuild-index` rebuilds the full-text index from the stored documents), `rm`, `done` / `done --undo`, `open` / `open --browser`, `export --format markdown|json`, `backup`, and `doctor` (`--json`): schema version, SQLite integrity check, search-index consistency, row counts, and unfinished background work, exiting 1 when something is wrong. `--data-dir` / `STOW_DATA_DIR` and `--allow-local-urls` / `STOW_ALLOW_LOCAL_URLS`. Exit code 3 means the bucket or item was not found.
- **MCP server** (`stw mcp`, crate `stow-mcp`): serves the library to Claude Desktop, Claude Code, or any MCP client over stdio with tools `search`, `list_buckets`, `list_items`, `get_item`, `add_links`, `move_item`, `archive_item`, `delete_item`, `update_notes`, `add_task`, `complete_task`, `add_reminder`, `create_bucket`, `export`, `doctor`, and the resources `stow://item/{id}` and `stow://bucket/{id}`. `--read-only` (or `STOW_MCP_READ_ONLY=1`) hides every tool that writes.
- **Danger zone** (Settings → Data): **Reset pairing code**, **Delete downloaded models (N MB)**, and **Delete all Stow data…**, which requires typing `delete`, empties the data folder, and restarts Stow with an empty library. An **About** footer shows the version, commit, and build date.
- **Responsive layout.** The list column scales with the window (380–620 px, 360 px below 1100 px wide), the window has a 900 × 600 minimum, and the Add link form hides its key hints on narrow lists.
- **Light** theme by default, with **Dark** and **System** options (Settings → General → Appearance).

### Changed
- CLI binary renamed from `stow` to `stw` so it never shadows GNU Stow.
- Default capture shortcut changed from ⌘⇧S to ⌃⌘S.
- "Done" is now called **Archive** throughout the app, with its own sidebar view and Restore.
- Tauri commands reject with structured `{ code, message }` errors instead of bare strings.
- Settings are grouped into General, Capture, Permissions, Chrome extension, AI, Transcription, and Data (with Advanced and Danger zone subsections and the About footer).
- Whisper model files are cached in memory between transcriptions instead of being reloaded every time.
- Snapshots are no longer attempted for video pages (YouTube, Vimeo, Loom, Twitch); the Page tab links to the video instead.
- The extension stores its pairing data in `chrome.storage.local` instead of `chrome.storage.sync` and migrates existing settings once; the `tabs` permission was dropped (`activeTab` is enough).
- API keys moved from Keychain service `stow` to `io.github.chaosdeselva.stow`; existing keys are migrated the first time they are read.
- The JSON export leaves out the extension token.

### Security
- **No requests to your local network.** Pages, images, and redirects are fetched only from public internet addresses: hostnames that resolve to loopback, private, link-local, or similar ranges are refused (including the cloud metadata address and IPv6 mapped forms), every connection's real peer address is checked, redirects are re-checked per hop and capped at 10, and system proxies are bypassed so they cannot undo these checks. Links to `localhost` or private addresses are still saved, just not fetched, unless you turn on **Fetch pages on private or local addresses** (Settings → Data → Advanced, restart required).
- **Bounded downloads and parsing.** Pages are read up to 10 MiB and images (favicons, previews) up to 5 MiB, aborting as soon as the limit is passed; only image content types are saved; HTML nested deeper than 512 levels is refused; parsing runs on its own thread with a large stack so a hostile page cannot crash the app.
- **Localhost endpoint hardening.** The extension endpoint binds to `127.0.0.1` only, requires a bearer token compared in constant time and re-read on every request (so **Reset pairing code** takes effect immediately), refuses requests whose `Host` header is not `127.0.0.1:<port>` or `localhost:<port>` (DNS-rebinding defense), limits bodies to 16 KiB, URLs to 8 KiB, and titles to 1 KiB, allows only `chrome-extension://` origins (optionally pinned to one extension id in Settings), and lets only one pairing request wait at a time, for at most 60 seconds.
- **Webview.** A Content Security Policy is enforced; the asset protocol can read only the data folder's `assets/` and `audio/` subfolders; the opener can open only `attachments/` and the data folder itself; navigation away from the app is blocked; `withGlobalTauri` is off; the capture window gets only the commands it needs.
- **Rendering.** Notes and AI output are sanitized with DOMPurify and never render images, forms, iframes, media, SVG, or MathML. Readable copies show remote images as links unless **Load images** is on.
- **Input validation.** All ids are checked as ULIDs and all enum-like settings (`theme`, `ai_provider`, `whisper_model`, hotkey, extension id) against allowlists at the command boundary, so a settings value can never become a file path.
- **Whisper models** download from a pinned whisper.cpp revision and are verified against a known size and SHA-256 before use; a mismatch deletes the file.
- **AI prompts** wrap page text and transcripts in untrusted blocks (truncated to 60,000 characters) and instruct the model to ignore instructions inside them; "Suggest tasks" is built from your notes and transcript, not from page text.
- **Chrome automation** passes URLs as script arguments, never interpolated into AppleScript, checks that Chrome is running with `pgrep` so it is never launched by accident, and closes a tab only if it still shows the captured URL.
- Data folder created with mode `0700`; `LSFileQuarantineEnabled` set so downloaded files carry the quarantine flag; the hardened-runtime entitlements are limited to microphone and Apple Events.

### Fixed
- Opening a database written by a newer Stow is refused with a clear error instead of running against an unknown schema.
- Captures and other jobs no longer stay "pending" forever or get lost at quit. A normal quit waits up to 5 seconds for running jobs and puts anything still running back to `pending`, so the next launch resumes the download or transcription; only work cut off by a crash is marked `interrupted`, and a snapshot or transcription that has failed three times stops being retried. `stw doctor` shows what is waiting.
- Adding a task, reminder, file, recording, or AI run to a bucket or link that was deleted meanwhile (for example from the CLI) is refused with a not-found error inside the same transaction, instead of leaving an orphan row or file behind.
- Importing hundreds of tabs no longer starts hundreds of simultaneous fetches: at most 4 network fetches, 1 transcription, and 2 AI calls run at once, and an identical job is never queued twice (**Download page again** / **Transcribe again** say it's already running instead).
- Reminder times are stored in canonical UTC so comparisons are correct regardless of the time zone they were entered in; existing rows are rewritten by the migration.
- Notes typed in the last moment before quitting are saved: the app waits for the frontend to flush before exiting.
- Switching items while recording still attaches the audio to the item you started on.
- Metadata fetches no longer replace a title you typed yourself.
- A default bucket deleted after the extension was configured falls back to the Inbox instead of failing.

[Unreleased]: https://github.com/ChaosDeSelva/Stow/commits/main
