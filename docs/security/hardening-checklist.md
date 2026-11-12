# Security hardening checklist

Maintainer checklist for shipping Stow safely. Ticked items are implemented in the tree as of 2026-11-08 (file references point at the code); anything unticked at release time is a known gap and goes in the release notes. The user-facing policy is [SECURITY.md](../../SECURITY.md); the fetch rules are explained in [docs/architecture/jobs-and-fetch.md](../architecture/jobs-and-fetch.md).

## Signing and notarization

- [ ] Sign with a **Developer ID Application** certificate and the **hardened runtime**; notarize with `notarytool` and **staple** the ticket to both `Stow.app` and the DMG. Verify with `spctl -a -t open --context context:primary-signature -v Stow.dmg` and `xcrun stapler validate` (`release.yml` does both once the secrets exist).
- [x] `apps/stow-app/src-tauri/Entitlements.plist` referenced from `bundle.macOS.entitlements`, containing **only** `com.apple.security.device.audio-input` and `com.apple.security.automation.apple-events`. Add `cs.allow-jit` / `cs.allow-unsigned-executable-memory` only if the ggml Metal library compile fails under the hardened runtime, and document why.
- [x] `LSFileQuarantineEnabled` in `Info.plist`, so files Stow writes from the network (snapshots, downloaded models) carry the quarantine flag.
- [x] Data directory created with mode `0700` (`db.rs::create_private_dir`, tested).
- [x] Bundle identifier decided (`io.github.chaosdeselva.stow`); permission grants and Keychain ACLs are keyed to it, so it must not change after the first public build.

## App Sandbox (evaluate; not required for Developer ID distribution)

- [ ] If enabling the sandbox: the data folder moves into the container (`~/Library/Containers/<id>/Data/Library/Application Support/Stow`), so ship a one-time migration from the old path.
- [ ] `osascript` is not available inside the sandbox; replace `Command::new("/usr/bin/osascript")` with `NSAppleScript` via objc bindings.
- [ ] Add `com.apple.security.temporary-exception.apple-events` for `com.google.Chrome`, and `network.client` for fetches and AI calls.

## Webview and IPC

- [x] A real CSP in `tauri.conf.json`: `default-src 'self'; img-src 'self' asset: http://asset.localhost data: blob:; media-src 'self' asset: http://asset.localhost; connect-src ipc: http://ipc.localhost; style-src 'self' 'unsafe-inline'; font-src 'self' data:; frame-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'`. No remote `script-src`; fonts and icons are self-hosted.
- [x] `app.withGlobalTauri: false`.
- [x] Asset protocol scope narrowed to `…/Stow/assets/**` and `…/Stow/audio/**`; opener `open-path` scope narrowed to `…/Stow/attachments/**` plus the data folder itself (for "Open in Finder"). No `$TEMP/**` or `/private/tmp/**`.
- [x] Per-window capabilities (`capabilities/main.json`, `capabilities/capture.json`): the `capture` window gets only the nine commands it uses plus event, basic window, and clipboard permissions. The JS global-shortcut permission was removed; Rust owns the shortcut.
- [x] `on_navigation` guard (`lib.rs::navigation_guard`) that blocks any navigation away from the app's own origin; external links go through the opener plugin.
- [x] Markdown rendering: DOMPurify with `FORBID_TAGS` for `img`, `form`, `input`, `button`, `iframe`, `object`, `embed`, `video`, `audio`, `svg`, `math` (and `style`/`onerror`/`onload` attributes) in notes and AI output; snapshot images rendered as links unless the per-item **Load images** toggle is on, and then with `referrerpolicy="no-referrer"` (`src/lib/markdown.ts`).
- [x] All ids validated as ULIDs at the command boundary (`error::require_id`); all enum-ish settings (`ai_provider`, `whisper_model`, `theme`, hotkey accelerator, `extension_id`) validated against allowlists before anything is written; export/backup destinations must be absolute.
- [x] Command errors are structured `{code, message}` and never contain keys, tokens, or page bodies.

## Fetching (SSRF and hostile pages)

- [x] One shared client for user-supplied URLs (`fetch::client()`): system proxies bypassed (`no_proxy`), `SafeResolver` drops non-public DNS answers, `PublicPeerLayer` checks the connected peer (catches literal IPs), redirects limited to `http`/`https`, non-local, 10 hops; 15 s timeout.
- [x] `is_public_ip` covers loopback, RFC 1918, link-local incl. `169.254.169.254`, CGNAT, multicast, broadcast, unspecified, documentation/benchmark/reserved, IPv6 ULA/link-local/site-local/Teredo/documentation, and IPv4-mapped, IPv4-compatible, NAT64 and 6to4 forms (unit-tested table).
- [x] Local URLs are saved but never fetched unless the user opts in (`allow_local_urls`, restart required; CLI `--allow-local-urls`). The opt-in client (`client_allow_local`) is the only path without guards and is what tests use.
- [x] Body caps: 10 MiB pages, 5 MiB assets, enforced on `Content-Length` and while streaming; asset extensions come only from an image `Content-Type` allowlist.
- [x] HTML nesting capped at 512 levels before readability/metadata parsing; parsing runs on a 64 MiB-stack thread so a hostile page cannot overflow a tokio worker.
- [x] Charset decoding uses `encoding_rs` with replacement, never panics on malformed bytes.
- [ ] Consider a per-host rate limit for refetch and import bursts (today: 4 concurrent fetches, no per-host limit).

## Localhost endpoint

- [x] Bind `127.0.0.1` only (never `0.0.0.0` or `::`), saved port → 47823 → random (`server.rs::bind_preferred`).
- [x] Bearer token required on everything except `/health` and `/pair`; 32 hex chars from `rand::random::<u128>()`; constant-time comparison (`subtle`); re-read on every request so **Reset pairing code** is immediate.
- [x] CORS allowlist of `chrome-extension://` origins; `extension_id` setting pins one exact origin (Settings → Chrome extension → **Extension ID (pins CORS)**). Pin the Web Store id there once it exists.
- [x] `/pair` requires in-app approval with a 60 s timeout, one pending request at a time (429 otherwise), and returns the current token only on **Allow**.
- [x] Reject requests whose `Host` header is not `127.0.0.1:<port>` or `localhost:<port>` (DNS-rebinding defense); JSON bodies capped at 16 KiB (413), URL at 8 KiB and title at 1 KiB (400), pair name at 100 chars; a deleted default bucket falls back to the Inbox.
- [x] Export JSON redacts `extension_token`.

## Updater (when enabled)

- [ ] `tauri-plugin-updater` with a **minisign** public key baked into the config; private key stored only in CI secrets.
- [ ] HTTPS-only endpoint (`github.com/ChaosDeSelva/Stow/releases/latest/download/latest.json`); `latest.json` and the `.app.tar.gz` are signed by the release job.
- [ ] Updater checks on launch and daily; never auto-installs without a prompt.

## Build and supply chain

- [x] Reproducible installs: `cargo … --locked`, `npm ci`; `Cargo.lock` and `package-lock.json` committed.
- [x] `cargo deny check` (advisories, licenses, bans, sources) and `cargo audit` on every push and weekly; `npm audit --omit=dev --audit-level=high` reported (advisory only until the tree is clean).
- [x] Dependabot for cargo, npm, and github-actions; Tauri Rust crate and `@tauri-apps/*` bumped together; `whisper-rs` minor bumps reviewed by hand.
- [ ] Generate a CycloneDX SBOM (`cargo cyclonedx`, `npx @cyclonedx/cyclonedx-npm`) and attach it to each release; publish **artifact attestations** (`actions/attest-build-provenance`) for the DMG, CLI tarball, and extension zip.
- [ ] Pin every GitHub Action to a commit SHA before 1.0 (currently pinned by major tag).
- [x] `SHA256SUMS` published with every release (`release.yml`).
- [ ] Secret scanning with push protection enabled on the repository; `gitleaks` run on the tree before the first push.
- [x] `GGML_NATIVE=OFF` in CI so release binaries never carry `-mcpu=native`.

## Whisper models

- [x] Download URLs pinned to a specific Hugging Face revision (`transcribe::MODEL_COMMIT`), not `main`.
- [x] Exact size and SHA-256 for `tiny.en`, `base.en`, and `small.en` recorded in `transcribe::MODELS`; verified while streaming; mismatch deletes the `.part`; `.part` files cleaned up on failure; concurrent downloads serialized.
- [x] Model names validated against the allowlist (`settings::WHISPER_MODELS`) before any path is built.

## Chrome extension

- [ ] Publish through the Chrome Web Store and pin the resulting extension ID in Settings → Chrome extension.
- [x] Token and settings in `chrome.storage.local` (never `storage.sync`); legacy `sync` records migrated once and removed.
- [x] Minimal permissions: `activeTab`, `contextMenus`, `storage`, host `http://127.0.0.1:*/*`. The `tabs` permission was dropped.
- [x] Only `http`/`https` URLs are sent; endpoint URLs built from a validated port (`lib.js::endpoint`), never string-interpolated; tokens validated as 32 hex chars.
- [ ] Longer term: move to Native Messaging ([ADR-0003](../architecture/adr/0003-localhost-extension-endpoint.md)) and close the port.

## Keychain

- [x] Keys stored under service `io.github.chaosdeselva.stow`, accounts `anthropic`/`openai` (legacy `stow` entries migrated); never in SQLite, settings, logs, job payloads, or events.
- [x] No command returns a key; `has_api_key` returns a boolean only.
- [x] Export and backup paths verified to contain no key material (keys are not in the data folder at all; the JSON export additionally redacts the extension token).

## Chrome automation

- [x] Chrome's presence is checked with `pgrep -x "Google Chrome"` before any AppleScript, so Stow never launches Chrome.
- [x] URLs and titles are passed to `osascript` as `argv`, never interpolated into script text.
- [x] A tab is closed only when its URL still matches the captured one (`close_front_tab({expectedUrl})`).
- [x] Permission status is read with `AEDeterminePermissionToAutomateTarget` without prompting; prompts happen only on explicit `request_automation` / first capture.
