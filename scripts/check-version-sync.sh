#!/usr/bin/env bash
# Fail unless every manifest agrees on the version (and matches $1 when given, e.g. a release tag).
# Usage: scripts/check-version-sync.sh [expected-version]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

json_version() { perl -0ne 'print $1 if /"version"\s*:\s*"([^"]*)"/' "$1"; }

cargo_v=$(perl -0ne 'print $1 if /\[workspace\.package\]\s*\nversion\s*=\s*"([^"]+)"/' "$ROOT/Cargo.toml")
pkg_v=$(json_version "$ROOT/apps/stow-app/package.json")
ext_v=$(json_version "$ROOT/extension/manifest.json")
tauri_v=""
if grep -q '"version"' "$ROOT/apps/stow-app/src-tauri/tauri.conf.json"; then
  tauri_v=$(json_version "$ROOT/apps/stow-app/src-tauri/tauri.conf.json")
fi

echo "Cargo.toml=$cargo_v package.json=$pkg_v extension=$ext_v tauri.conf.json=${tauri_v:-<inherits Cargo>}"

ok=1
[[ "$cargo_v" == "$pkg_v" ]] || { echo "drift: package.json ($pkg_v) != Cargo.toml ($cargo_v)" >&2; ok=0; }
[[ "${cargo_v%%-*}" == "$ext_v" ]] || { echo "drift: extension/manifest.json ($ext_v) != ${cargo_v%%-*}" >&2; ok=0; }
if [[ -n "$tauri_v" && "$tauri_v" != "$cargo_v" ]]; then
  echo "drift: tauri.conf.json ($tauri_v) != Cargo.toml ($cargo_v)" >&2; ok=0
fi
if [[ -n "${1:-}" && "$1" != "$cargo_v" ]]; then
  echo "expected $1 but manifests say $cargo_v" >&2; ok=0
fi
[[ $ok == 1 ]] || exit 1
echo "versions in sync"
