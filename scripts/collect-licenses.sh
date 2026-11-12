#!/usr/bin/env bash
# Generates THIRD_PARTY_LICENSES.md: every Rust crate and npm package shipped in Stow.app / stw,
# with license texts, plus components outside both package managers (whisper.cpp, SQLite).
#
#   scripts/collect-licenses.sh          regenerate THIRD_PARTY_LICENSES.md in place
#   scripts/collect-licenses.sh --check  regenerate to a temp file and fail if it differs
#
# Needs: cargo-about (`cargo install cargo-about --locked --features cli`), node, jq, and an
# installed apps/stow-app/node_modules (`npm ci`). Output is deterministic (sorted, no dates) so
# CI can diff it; THIRD_PARTY_NOTICES.md explains how both files are delivered to users.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$ROOT/apps/stow-app"
OUT="$ROOT/THIRD_PARTY_LICENSES.md"
CONFIG="$ROOT/scripts/licenses/about.toml"
RENDER="$ROOT/scripts/licenses/render.mjs"

check=0
[ "${1:-}" = "--check" ] && check=1

for tool in cargo node jq npm; do
  command -v "$tool" >/dev/null || { echo "collect-licenses: missing $tool" >&2; exit 1; }
done
if ! cargo about --version >/dev/null 2>&1; then
  echo "collect-licenses: cargo-about not installed. Run: cargo install cargo-about --locked --features cli" >&2
  exit 1
fi
if [ ! -d "$APP/node_modules" ]; then
  echo "collect-licenses: $APP/node_modules missing. Run: (cd apps/stow-app && npm ci)" >&2
  exit 1
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# --workspace so stow-app's feature set (metal, audio, transcribe, ai, keychain) is included;
# --locked so the result reflects Cargo.lock exactly. about.toml drops build/dev dependencies.
(cd "$ROOT" && cargo about generate --format json --workspace --locked -c "$CONFIG" -o "$tmp/about.json")

node "$RENDER" "$tmp/about.json" "$ROOT" "$APP" "$tmp/THIRD_PARTY_LICENSES.md"

# Sanity: the components the notices promise must be present.
for needle in "whisper.cpp" "Font Awesome" "ibm-plex-sans" "dompurify" "marked" "tauri"; do
  grep -qi -- "$needle" "$tmp/THIRD_PARTY_LICENSES.md" || {
    echo "collect-licenses: generated file does not mention '$needle'" >&2; exit 1; }
done

if [ "$check" = 1 ]; then
  if [ ! -f "$OUT" ]; then
    echo "collect-licenses: $OUT is missing; run scripts/collect-licenses.sh and commit it" >&2
    exit 1
  fi
  if ! diff -u "$OUT" "$tmp/THIRD_PARTY_LICENSES.md"; then
    echo "collect-licenses: THIRD_PARTY_LICENSES.md is stale; run scripts/collect-licenses.sh (just licenses) and commit" >&2
    exit 1
  fi
  echo "collect-licenses: THIRD_PARTY_LICENSES.md is up to date"
else
  mv "$tmp/THIRD_PARTY_LICENSES.md" "$OUT"
  echo "collect-licenses: wrote $OUT"
fi
