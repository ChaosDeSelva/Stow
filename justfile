# Stow task runner. Install just with `brew install just`; run `just` to list recipes.
set shell := ["bash", "-euo", "pipefail", "-c"]

app := "apps/stow-app"

default:
    @just --list

# One-time machine setup: Rust components, cmake for whisper.cpp, Node deps.
setup:
    rustup show active-toolchain >/dev/null || rustup toolchain install stable
    rustup component add rustfmt clippy
    brew list cmake >/dev/null 2>&1 || brew install cmake
    cd {{app}} && npm ci

# Run the app against a scratch data folder (override with STOW_DATA_DIR).
dev:
    cd {{app}} && STOW_DATA_DIR="${STOW_DATA_DIR:-$HOME/Library/Application Support/Stow-dev}" npm run tauri dev

# All tests: Rust workspace + Vitest.
test:
    cargo test --workspace --locked
    cd {{app}} && npm test

# Static checks: rustfmt, clippy, svelte-check, extension syntax.
lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cd {{app}} && npm run check
    node --check extension/background.js extension/options.js extension/lib.js
    node --test 'extension/*.test.mjs'

# Format Rust (frontend formatting is Prettier with the repo .prettierrc, if installed).
fmt:
    cargo fmt --all
    cd {{app}} && (npx --no-install prettier --write src >/dev/null 2>&1 || true)

# Release build of the app (Apple Silicon). src-tauri is a workspace member, so the output lands
# in the workspace target dir: target/aarch64-apple-darwin/release/bundle/{macos,dmg}.
build:
    cd {{app}} && npm run tauri build -- --target aarch64-apple-darwin

# Everything CI gates on, plus supply-chain checks, version sync, and the licenses file.
release-check: lint test
    cd {{app}} && npm run build
    cargo build --release --locked -p stow-cli && target/release/stw --help >/dev/null
    cargo deny check
    cargo audit
    scripts/check-version-sync.sh
    scripts/collect-licenses.sh --check

# Regenerate THIRD_PARTY_LICENSES.md from Cargo.lock + node_modules (needs cargo-about:
# `cargo install cargo-about --locked --features cli`). Commit the result. `just licenses check`
# only verifies the committed file is current (CI does this).
licenses mode="":
    scripts/collect-licenses.sh {{ if mode == "check" { "--check" } else { "" } }}

# Zip the Chrome extension into out/.
package-extension out="out":
    scripts/package-extension.sh {{out}}

# Set the version in every manifest (see RELEASING.md).
set-version v:
    scripts/sync-version.sh {{v}}

# Run the CLI from source: just cli ls
cli *ARGS:
    cargo run -q -p stow-cli -- {{ARGS}}

# Remove build output.
clean:
    cargo clean
    rm -rf {{app}}/dist {{app}}/node_modules out
