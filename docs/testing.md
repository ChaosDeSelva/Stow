# Testing

How Stow is tested, what runs in CI, what can't run in CI, and the backlog of tests worth adding. Condensed from the 2026-11-08 testing and release-engineering review.

## What runs where

| Layer | Runner | Isolation | Where it runs |
|---|---|---|---|
| `stow-core` unit and integration tests | `cargo test` | `tempfile` data dirs; HTTP via `wiremock`; no Keychain, microphone, or real network | `macos-latest` (Metal, CoreAudio, AppKit are needed to link) |
| `stow-cli` integration tests | `cargo test` + `assert_cmd` | Binary run against a temp `STOW_DATA_DIR`; `--no-fetch` or wiremock | `macos-latest` |
| `stow-app` Rust tests (server, settings validation, chrome script building, shortcut parsing, scheduler helpers) | `cargo test` | Loopback ports; no window | `macos-latest` |
| Frontend | Vitest + jsdom, real Svelte 5 mounts | `@tauri-apps/api` mocked | CI runs on `macos-latest` alongside the build; it would also pass on Linux |
| Extension | `node --test 'extension/*.test.mjs'` against `lib.js` | No Chrome APIs | `ubuntu-latest` |
| Supply chain | `cargo deny check`, `cargo audit`, `npm audit --omit=dev` | — | with the Rust and frontend jobs |
| Version sync | `scripts/check-version-sync.sh` | — | `ubuntu-latest` |

Local shortcut: `just lint && just test`, or `just release-check` for everything CI gates on.

Environment knobs:
- `STOW_WHISPER_CPU=1` keeps whisper.cpp on the CPU path (CI runners have no Metal device; this also silences the ggml device-init log spam).
- `STOW_DATA_DIR` points core, CLI, and app at a scratch folder.
- The `#[ignore]`d `transcribe_real_model` test needs `STOW_WHISPER_MODEL=<path to ggml-tiny.en.bin>` and `STOW_TEST_WAV=<16 kHz mono wav>`. Synthesize one with `say -o fox.aiff "The quick brown fox" && afconvert -f WAVE -d LEI16@16000 -c 1 fox.aiff fox.wav`.

## Rules

- Nothing in the test suite touches the real Keychain. It prompts or hangs on a headless runner. Core accepts a key directly so tests bypass it.
- Nothing hits the real network. Fetch functions take a `&reqwest::Client`, so a wiremock base URL or a short-timeout client is cheap to inject.
- Tests that bind loopback ports (`server::tests::start_*`) or stream multi-megabyte bodies (`download_asset_rejects_*`) are the flaky-risk set. Keep them few and give them generous timeouts; if they start flaking, move to `cargo nextest` with a single-threaded `net` test group.
- `time.test.ts` depends on local time. Run Vitest with `TZ=UTC` and `TZ=America/Los_Angeles` when touching date code.

## End-to-end testing of the built app: not feasible on macOS

`tauri-driver` wraps WebKitWebDriver on Linux and msedgedriver on Windows. There is no WebDriver for WKWebView on macOS, and `safaridriver` cannot attach to a Tauri app. Don't budget CI time for driving `Stow.app`.

What works instead:

1. **Headless contract tests of the real Rust commands** (recommended). Enable `tauri = { version = "2", features = ["test"] }` under `[dev-dependencies]`, move the `generate_handler![…]` list into a `pub fn handler()`, build the app with `tauri::test::mock_builder()`, `app.manage(AppState { … })` with a tempdir `Db`, and drive each command with `tauri::test::get_ipc_response(&webview, InvokeRequest { cmd, body: InvokeBody::Json(fixture), … })`. This runs Tauri's real camelCase-to-snake_case argument conversion and the real command bodies with no window, so it is the one place the TS↔Rust contract is proven end to end. Check the exact `InvokeRequest` fields against the Tauri 2 version in `Cargo.lock` when implementing.
2. **Shared JSON fixtures** under `apps/stow-app/contract/fixtures/*.json`, one per command, asserted from both sides: Vitest checks that `api.ts` produces the fixture; Rust deserializes it into the command's parameter types.
3. **Browser-level smoke test** with Playwright against `vite preview`, using `@tauri-apps/api/mocks` (`mockIPC`, `mockWindows("main")`) seeded with fixtures. Covers hash routing (`#/capture`), the capture panel, the settings form, and toasts without Tauri. Runs on Linux. Make it a separate, non-required job first.
4. **Manual release checklist** on a clean user account for the things only a signed bundle exercises: the Automation prompt, microphone prompt, a reminder notification, Keychain access (see RELEASING.md).

## Backlog: the 15 most valuable tests to add

Ordered roughly by value. Items marked **(bug)** fail against the code as reviewed and document a fix.

1. **IPC contract fixtures, both sides.** `apps/stow-app/contract/fixtures/*.json` (`add_item`, `update_bucket`, `import_chrome_tabs`, `update_settings`, `reminder_agenda`, …). Vitest: `api.addItem(...)` produces the fixture. Rust: `serde_json::from_value` into `BucketPatch`, `SettingsPatch`, `Vec<ChromeTab>`, `ItemFilter`, enums succeeds. Catches camelCase/snake_case drift in nested structs, which Tauri does not convert.
2. **`versions_are_in_sync`** (stow-app unit test): `include_str!` of `tauri.conf.json`, `package.json`, and `extension/manifest.json` all equal `env!("CARGO_PKG_VERSION")`. Complements `scripts/check-version-sync.sh`.
3. **Two DB handles interleave writes without `SQLITE_BUSY`** (`db.rs`): open the same dir twice (CLI + app), two threads doing 200 writes each, assert all rows present and `PRAGMA journal_mode = wal`. Plus a CLI test holding a write transaction for 1 s while `stw add --no-fetch` runs.
4. ~~**Migrations apply only pending and refuse a newer schema** (`db.rs`)~~ **Done**: `Db::open` fails with `StowError::SchemaTooNew` (code `schema`); covered by `schema_too_new_is_rejected` and `migration_chain_applies_from_empty_and_from_v1`.
5. **`import_chrome_tabs` counts new, duplicate, and invalid** (`commands/chrome.rs`): `[valid, same-valid, chrome://newtab]` gives `imported 1, skipped 2`, and only the two valid tabs are handed to close. Needs the loop body refactored into a plain function.
6. **`reminder_agenda` partitions and doesn't starve `upcoming`** **(bug)**: 3 overdue, 2 today, 250 future must yield 50 upcoming; the current `list_upcoming(200)`-then-filter approach returns 0 once overdue/today exceed 200.
7. ~~**`update_settings` rejects unknown provider and model names**~~ **Done**: `settings::validate_whisper_model` / `validate_ai_provider` / `validate_theme` allowlists, covered by `settings_validation_rejects_bad_values_with_invalid_input` and the core `validation_helpers` test.
8. **Metadata and snapshot fetches time out instead of hanging**: a header-only stalling server and a client with `.timeout(300ms)` yield `StowError::Http` within 1 s and `snapshot_status = failed`. Closes the "offline" case of the review focus.
9. **Model download: 404 and truncated downloads remove the `.part` file** (`transcribe.rs`): wiremock 404 → `Err(Transcribe)` and no `.part`; `Content-Length: 1000` with 10 bytes sent → "incomplete download", no `.part`, and the next call retries. (Checksum mismatch and concurrent-download cases are covered; 404 and truncation are not yet.)
10. **Search neutralizes FTS operators** (`repo/search.rs`): `foo:bar`, `a*`, `NEAR(foo)`, `-baz`, `foo^`, `"`, `(`, `AND` all return `Ok`, and `foo:bar` finds the indexed doc. Also `limit` honoured and stable ordering on a 5 000-item index.
11. **Scheduler uses the fallback title when the target is gone** (`scheduler.rs`): a reminder whose parent was removed still fires once with "Saved link"/"Bucket" and is marked fired.
12. **Chrome is never addressed before the System Events running check** (`chrome.rs`): introduce a `ScriptRunner` trait with a recording fake; for `front_tab`, `all_tabs`, `close_front_tab`, `close_tabs`, the first script contains `"System Events"`, and when it returns `"false"` no script containing `"Google Chrome"` runs. Makes the "never launch Chrome" guarantee executable.
13. **`renderMarkdown` sanitization** (`markdown.ts`): `<img onerror>`, `javascript:` links, `<script>`, `<iframe>` are stripped; GFM task lists and tables survive; `parseTaskLines` handles `- [ ]`, `- [x]`, numbered lists, bold, CRLF. This is the XSS boundary.
14. **`friendlyError` mapping and the `CapturePanel` flow**: each contract error code maps to copy; mounting `CapturePanel`, a `capture-open` event with `capture_front_tab` rejecting `permission_denied` shows the Automation copy and the paste field; on success with close-tab enabled it calls `close_front_tab` with `expectedUrl` equal to the captured URL.
15. **Extension service worker** (`background.js` with a `chrome` stub and `fetch` mock): unpaired → options page opened, no fetch; `chrome://` tab → ✕ badge, no fetch; success → `POST` body `{url, title, bucket_id}` with `Bearer` header, ✓ badge, `tabs.remove` only when close-tab is on and it isn't a link capture; HTTP 401 → ✕ badge, no `tabs.remove`.

Honourable mentions: the `JobQueue` per-kind concurrency caps and dedupe (now implemented: 4 network / 1 transcribe / 2 AI, identical jobs refused; `resume_pending` and `interrupted` marking are covered, the slot limits are not asserted), the SSRF guards in `fetch/net.rs` against a redirect chain that ends on a private address (the per-hop policy and `is_public_ip` table are unit-tested; an end-to-end redirect test through the real client is not), `attachments::add` on a directory, `stop_recording` when the DB insert fails leaves no orphan WAV, `RecorderHandle::start` with no input device (exactly what CI has), `stw mv` with an ambiguous prefix, and the server's CORS predicate.

## Coverage snapshot (2026-11-08, after the hardening pass)

The counts in the first draft of this page (113 Rust, 53 Vitest) are stale; the hardening pass added tests for SSRF (`is_public_ip` table, local-host detection, redirect policy, the safe client refusing a private literal IP), size caps (header and streaming), nesting guard, charset decoding, model checksum and `.part` cleanup, `resume_pending`, schema-too-new, concurrent app+CLI writers, migration chain equality, settings validation, server host/token/CORS predicates, and the CLI commands `rm`/`done`/`open`/`export`/`backup`/exit code 3. Run `cargo test --workspace --locked 2>&1 | grep "test result"` and `npm test` for current numbers. Still thin: the Tauri command layer end to end (`commands/*` through `tauri::test`), the `scheduler.rs` tick loop, the markdown sanitizer, and the extension service worker.
