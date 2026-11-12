# Contributing to Stow

Thanks for helping. Stow is small and opinionated: local-first, macOS-native, and no network on the capture path. Before starting anything bigger than a bug fix, open an issue or Discussion so we can agree on the shape first.

## Dev setup

```bash
xcode-select --install                 # Apple toolchain
curl https://sh.rustup.rs -sSf | sh    # Rust; rust-toolchain.toml pins the channel and adds rustfmt + clippy
brew install cmake node@22 just        # cmake builds whisper.cpp; Node 22 LTS (.nvmrc); just is optional
git clone https://github.com/ChaosDeSelva/Stow && cd Stow
just setup                             # or: cd apps/stow-app && npm ci
```

Run the app against a scratch data folder so you don't touch your real library:

```bash
just dev                                            # uses ~/Library/Application Support/Stow-dev
STOW_DATA_DIR="$(mktemp -d)" npm run tauri dev      # from apps/stow-app, fully disposable
```

`STOW_DATA_DIR` is the one knob for the data folder: the app, the `stw` CLI, and `stow-mcp` all read it, so point them at the same scratch folder to test them together. The default (no variable) is `~/Library/Application Support/Stow`, your real library.

### Developing alongside the installed app

Stow uses `tauri-plugin-single-instance`, keyed on the bundle identifier `io.github.chaosdeselva.stow`. The dev build and the installed `/Applications/Stow.app` share that identifier, so:

- **Quit the installed Stow before `just dev`.** Otherwise the dev binary just forwards its launch to the running copy and exits (or the installed one grabs the global hotkey and the extension port first).
- **Turn off "Open at login" in the dev build before quitting it.** The autostart plugin registers whichever binary is running, so enabling it from `tauri dev` points the login item at `target/debug/stow-app`. Disable it in Settings before you stop dev, or you will get a debug build at your next login. The installed app re-registers itself when you toggle the setting there.
- macOS permission grants (Automation, Microphone, Notifications) are tied to the identifier plus the signing identity, so the dev build will prompt again even if the installed app was already allowed.

Things that only behave correctly in a bundled build (`npm run tauri build -- --debug`):

- macOS attributes Automation, Microphone, and Notification prompts to your terminal in `tauri dev`. Test those flows from the `.app`.
- Notifications may not appear at all from an unbundled binary.
- Keychain prompts differ between unsigned and signed builds.

The first `cargo` build compiles whisper.cpp and takes a few minutes. Later builds are incremental.

## Tests and checks

Run these before opening a PR. CI runs the same commands (`.github/workflows/ci.yml`).

```bash
just lint && just test          # or, by hand:
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cd apps/stow-app && npm test && npm run check && npm run build
node --check extension/background.js extension/options.js extension/lib.js
node --test 'extension/*.test.mjs'
```

- **Core tests** use a temporary data folder (`tempfile`), and HTTP is mocked with `wiremock`. Tests never hit the real network, Keychain, or microphone.
- **CLI tests** run the `stw` binary with `assert_cmd` against a temporary `STOW_DATA_DIR`.
- **App Rust tests** cover the localhost server (loopback ports), settings validation, Chrome script handling, and shortcut parsing without opening a window.
- **Frontend tests** use Vitest with `@tauri-apps/api` mocked; component tests mount real Svelte 5 components in jsdom.
- **Extension tests** are plain `node --test` files against the pure helpers in `extension/lib.js`.

The test strategy and backlog are in [docs/testing.md](docs/testing.md).

## Code style

**Rust**
- `rustfmt` with the repo `rustfmt.toml` (`max_width = 100`) and clippy-clean code with `clippy::pedantic` warnings enabled in the workspace. `unsafe_code` is forbidden everywhere except `src-tauri/src/permissions.rs` (macOS permission FFI).
- Core returns `stow_core::Result<T>` with `StowError`, whose `code()` is stable. Tauri commands convert errors into structured `CmdError { code, message }` values (see [ADR-0007](docs/architecture/adr/0007-structured-command-errors.md)). Don't `unwrap()` outside tests.
- `stow-core` must not depend on Tauri or any UI crate. Features: `audio`, `transcribe`, `ai`, `keychain` (default), `metal` (enabled by the app). The CLI builds core with `default-features = false`.
- Comments explain *why*, not what.

**TypeScript and Svelte**
- Svelte 5 runes and strict TypeScript; keep `svelte-check` clean.
- Every `invoke()` goes through `src/lib/api.ts`. Components never call `invoke` directly.
- State lives in one `.svelte.ts` store per domain under `src/lib/stores/`.
- Double quotes and semicolons (the repo `.prettierrc` matches).

**CSS**
- Use the tokens defined in `src/app.css`. Light theme is the default; dark tokens live under `[data-theme="dark"]`. No drop shadows. See [docs/design/DESIGN.md](docs/design/DESIGN.md) for the palette reference.

**Privacy rule.** Capturing a link must never wait on the network. Anything that talks to the internet runs as a background job through `fetch::client()` (which carries the SSRF guards; see [docs/architecture/jobs-and-fetch.md](docs/architecture/jobs-and-fetch.md)), and anything that sends user content off the machine is triggered by an explicit user action and listed in the README privacy table.

## Adding a Tauri command end to end

The frontend and backend agree on a written contract: [`docs/superpowers/plans/tauri-contract.md`](docs/superpowers/plans/tauri-contract.md) (also reachable as [docs/architecture/ipc-contract.md](docs/architecture/ipc-contract.md)). Rust command names are `snake_case`. **JS argument keys are camelCase** (Tauri converts `bucket_id` to `bucketId`). **Returned fields stay snake_case**, exactly as in `models.rs`.

1. **Core.** Add the logic as a plain function in `crates/stow-core/src/repo/<domain>.rs` (or the relevant module) with a unit test. If it changes something searchable, upsert the row in `search_docs` in the same transaction (`search::index_conn`); the FTS5 index follows by trigger. See [docs/architecture/data-model.md](docs/architecture/data-model.md).
2. **Command.** Add it in `apps/stow-app/src-tauri/src/commands/<domain>.rs`:
   ```rust
   #[tauri::command]
   pub async fn rename_task(state: State<'_, AppState>, id: String, title: String) -> CmdResult<Task> {
       require_id("id", &id)?;
       db(&state, move |d| tasks::rename(d, &id, &title)).await
   }
   ```
   Validate ids (`require_id`) and user input at the boundary (`invalid_input`). Anything that blocks (database, files, Keychain, `osascript`, the recorder) goes through `db(...)` or `blocking(...)` so it never runs on the main thread.
3. **Register** it in `generate_handler![…]` in `src-tauri/src/lib.rs`, and build once: Tauri generates `permissions/autogenerated/<command>.toml`.
4. **Capabilities.** Add `allow-<command-name>` to `src-tauri/capabilities/main.json`. Add it to `capture.json` only if the quick-capture panel needs it; that window is deliberately limited. New *plugin* APIs get the narrowest permission in the same files.
5. **Contract.** Add a line to the contract: name, JS args, return type, error codes. If it emits an event, add it to the Events section.
6. **Types and API.** Mirror any new struct in `src/lib/types.ts`, then add a wrapper in `src/lib/api.ts`:
   ```ts
   export const renameTask = (id: string, title: string) => invoke<Task>("rename_task", { id, title });
   ```
7. **Store and UI.** Call the wrapper from the domain store, and add a Vitest test for the store logic.

## Adding a database migration

Migrations are numbered SQL files applied in order at startup, each inside a transaction, with `PRAGMA user_version` recording how many have run (`crates/stow-core/src/db.rs`).

1. Create `crates/stow-core/migrations/000N_<what>.sql`. Don't include `BEGIN`/`COMMIT`; the runner wraps the file.
2. Append it to the `MIGRATIONS` list in `db.rs`. If the migration needs Rust-side data fixes (like 0002's reminder-time normalization), hook them in `migrate()` by number.
3. Update `models.rs`, the repo functions, `types.ts`, and the contract.
4. Add a test that opens a database at the previous version (`Db::open_at`) and checks the upgrade, and keep `migration_chain_applies_from_empty_and_from_v1` green: a fresh database and an upgraded one must end with the same tables, indexes, and triggers.
5. Polymorphic parent columns (`parent_type`/`parent_id`, `target_type`/`target_id`) are not foreign keys. Row cleanup is done by the `*_cascade` `AFTER DELETE` triggers; if you add a table that points at a parent, extend those triggers in your migration and add the file cleanup to `OwnedFiles` in `repo/items.rs` ([ADR-0006](docs/architecture/adr/0006-polymorphic-parents-with-triggers.md)).

Migrations are **forward-only and immutable once released**. Never edit a shipped file; add a new one. Users' data depends on it. Opening a database with a higher `user_version` than the binary knows fails with the `schema` error code.

## The CLI

The Cargo package is `stow-cli`; the installed binary is `stw` (so it never shadows GNU Stow's `stow`). Keep `#[command(name = "stw")]`, the `[[bin]]` name, and `tests/cli.rs` in agreement. Exit codes: 0 success, 1 any error, 3 "bucket or item not found".

## Dependencies and licenses

`THIRD_PARTY_LICENSES.md` is generated from `Cargo.lock` and `apps/stow-app/node_modules` and committed. After adding or bumping a dependency, run `just licenses` (needs `cargo install cargo-about --locked --features cli --version 0.9.3`) and commit the result; CI runs `scripts/collect-licenses.sh --check` and fails if the file is stale. A new license kind must be added to both `deny.toml` (`[licenses].allow`) and `scripts/licenses/about.toml`, and anything user-visible (a font, an icon set, a native library) gets a row in `THIRD_PARTY_NOTICES.md`.

## Commits and pull requests

- Use [Conventional Commits](https://www.conventionalcommits.org): `feat(capture): …`, `fix(core): …`, `docs: …`. Scopes are `core`, `cli`, `app`, `ui`, `ext`, `docs`, `ci`.
- **Sign off every commit** (`git commit -s`). Stow uses the [Developer Certificate of Origin](https://developercertificate.org): your sign-off certifies you have the right to submit the change under the project's license. There's no CLA.
- Keep each PR to one concern. Include tests, update `CHANGELOG.md` under *Unreleased* for user-visible changes, and add a screenshot for UI changes.
- PRs are squash-merged. The PR title becomes the commit message, so make it a good Conventional Commit.

## Architecture decisions

Significant decisions are recorded as ADRs in [`docs/architecture/adr`](docs/architecture/adr). If your change reverses or extends one, add a new ADR in the same PR.
