# Releasing Stow

One tag produces three artifacts: `Stow_<v>_aarch64.dmg` (signed and notarized), `stow-cli_<v>_aarch64-apple-darwin.tar.gz` (the `stw` binary), and `stow-extension_<v>.zip`.

## Versioning

- [Semantic Versioning](https://semver.org). Pre-1.0: a MINOR bump may change the data layout, but only through a new file in `crates/stow-core/migrations/`; a PATCH never changes the schema.
- One version string lives in four manifests: `Cargo.toml` (`[workspace.package].version`, inherited by every crate), `apps/stow-app/src-tauri/tauri.conf.json`, `apps/stow-app/package.json` (+ lock), and `extension/manifest.json`. `scripts/sync-version.sh <version>` sets all of them; `scripts/check-version-sync.sh` fails CI when they drift.
- Chrome Web Store versions must be 1–4 dot-separated integers. App pre-releases are tagged `v0.2.0-rc.1`; the extension keeps `0.2.0` and is not uploaded to the store for pre-releases.

## CHANGELOG

`CHANGELOG.md` follows [Keep a Changelog 1.1](https://keepachangelog.com/en/1.1.0/). Every user-visible PR adds a line under `## [Unreleased]` in one of Added, Changed, Deprecated, Removed, Fixed, Security. At release time the section becomes `## [X.Y.Z] - YYYY-MM-DD` and the compare links at the bottom are updated. `scripts/changelog-section.sh X.Y.Z` prints that section; `release.yml` uses it as the GitHub Release body.

## Cutting a release

1. On `main`, make sure `just release-check` is green (fmt, clippy, tests, svelte-check, Vitest, build, CLI smoke test, cargo deny, cargo audit, version sync, and `scripts/collect-licenses.sh --check`). If the licenses check fails, run `just licenses` and commit `THIRD_PARTY_LICENSES.md`; it must match the lockfiles the tag is built from.
2. `scripts/sync-version.sh 0.2.0`, then move `[Unreleased]` to `## [0.2.0] - <today>` in `CHANGELOG.md` and add the compare links.
3. Commit and tag:
   ```bash
   git commit -s -am "release: v0.2.0"
   git tag -a v0.2.0 -m "Stow v0.2.0"
   git push origin main v0.2.0
   ```
4. `release.yml` verifies the tag matches the manifests, builds on `macos-14`, signs with the Developer ID certificate, notarizes with `notarytool`, staples the ticket, and uploads the DMG, `SHA256SUMS`, the CLI tarball, and the extension zip to a **draft** release. The CLI tarball contains `stw`, Stow's `LICENSE*` files, `THIRD_PARTY_NOTICES.md`, and `THIRD_PARTY_LICENSES.md`; the workflow also fails if those two notice files are missing from `Stow.app/Contents/Resources` (they are bundled through `bundle.resources` in `tauri.conf.json`). See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for why.
5. Download the DMG from the draft and test on a clean user account: hotkey capture from Chrome (the Automation prompt appears exactly once), the microphone prompt, a reminder firing, and `spctl -a -t open --context context:primary-signature -v Stow.dmg` reporting `accepted`.
6. Publish the draft. Then bump the Homebrew cask and upload the extension (below), and start a fresh `## [Unreleased]`.

## Where the build output lands

`apps/stow-app/src-tauri` is a member of the root Cargo workspace, so cargo writes everything to the **workspace** `target/`, not `apps/stow-app/src-tauri/target`:

- `just build` / `npm run tauri build -- --target aarch64-apple-darwin` → `target/aarch64-apple-darwin/release/bundle/macos/Stow.app` and `target/aarch64-apple-darwin/release/bundle/dmg/Stow_<v>_aarch64.dmg`
- a debug bundle (`-- --debug`) → `target/aarch64-apple-darwin/debug/bundle/...`
- the CLI → `target/release/stw` (or `target/aarch64-apple-darwin/release/stw` with an explicit `--target`)

`release.yml` reads the DMG from the workspace path and only falls back to `src-tauri/target` for old tags.

## Notarization prerequisites

One-time, before the first public release:

- An Apple Developer Program membership and a **Developer ID Application** certificate.
- `apps/stow-app/src-tauri/Entitlements.plist` with `com.apple.security.device.audio-input` and `com.apple.security.automation.apple-events`, referenced from `bundle.macOS.entitlements` in `tauri.conf.json`. Without them a hardened-runtime build cannot record or talk to Chrome, although `tauri dev` works.
- `Info.plist` carries `NSMicrophoneUsageDescription` and `NSAppleEventsUsageDescription` (it does).
- Repository secrets (GitHub → Settings → Secrets → Actions):

| Secret | What |
|---|---|
| `APPLE_CERTIFICATE` | base64 of the Developer ID Application `.p12` (`base64 -i cert.p12 \| pbcopy`) |
| `APPLE_CERTIFICATE_PASSWORD` | the `.p12` export password |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Name (TEAMID)` |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Apple ID, an **app-specific password**, and the Team ID for `notarytool` |
| `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | only once the updater plugin is enabled (`npm run tauri signer generate -- -w ~/.tauri/stow.key`) |

Decide the bundle identifier before the first release. macOS ties Automation, Microphone, and Notifications grants and Keychain access to it (plus the signing team), so changing it later forces every user to re-grant.

## Chrome Web Store

- `scripts/package-extension.sh out` produces `out/stow-extension_<v>.zip` with `manifest.json` at the zip root; `lib.js` is included, `lib.test.mjs`, `README.md`, and dotfiles are excluded. CI builds it on every extension change so it never rots.
- The listing needs a 128×128 icon, at least one 1280×800 or 640×400 screenshot, a single-purpose description ("sends the current tab to the Stow app on this Mac"), a privacy policy URL (the extension talks only to `http://127.0.0.1:<port>`), and a justification for `host_permissions: http://127.0.0.1:*/*`.
- Upload manually through the Developer Dashboard for now. Once the listing exists, pin the store extension ID in Settings → Chrome extension → **Extension ID (pins CORS)** (the `extension_id` setting; empty allows any `chrome-extension://` origin) and consider moving to Native Messaging ([ADR-0003](docs/architecture/adr/0003-localhost-extension-endpoint.md)).

## Homebrew (later)

GNU Stow owns the `stow` formula name, so publish a cask `stow-app` in a tap (`github.com/ChaosDeSelva/homebrew-stow`) pointing at the release DMG, with `depends_on arch: :arm64` and `depends_on macos: ">= :ventura"`, and a `zap trash` entry for `~/Library/Application Support/Stow`. A `stow-cli` formula can install the `stw` binary from the release tarball. Bump with `brew bump-cask-pr`.

## Updater (later)

Add `tauri-plugin-updater` and `tauri-plugin-process`, set `bundle.createUpdaterArtifacts: true`, `plugins.updater.endpoints` to `https://github.com/ChaosDeSelva/Stow/releases/latest/download/latest.json`, and the minisign public key from `tauri signer generate`. Set `includeUpdaterJson: true` in `release.yml` and add the signing-key secrets. Stow stays running for days, so check on launch and once a day from the scheduler tick, and add a "Check for Updates…" tray item.
