#!/usr/bin/env bash
# Zip extension/ for the Chrome Web Store and GitHub Releases.
# Usage: scripts/package-extension.sh [out-dir]   (default: out)
# Produces <out-dir>/stow-extension_<version>.zip with manifest.json at the zip root
# and no dotfiles, tests, or docs.
set -euo pipefail
OUT="${1:-out}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
V=$(perl -0ne 'print $1 if /"version"\s*:\s*"([^"]*)"/' "$ROOT/extension/manifest.json")
[[ -n "$V" ]] || { echo "could not read version from extension/manifest.json" >&2; exit 1; }

mkdir -p "$OUT"
ZIP="$(cd "$OUT" && pwd)/stow-extension_${V}.zip"
rm -f "$ZIP"
(
  cd "$ROOT/extension"
  zip -r -X -q "$ZIP" . -x '.*' '*/.*' '*.map' '*.test.js' '*.test.mjs' 'README.md' 'vitest.config.*'
)
unzip -l "$ZIP" | grep -q ' manifest.json$' || { echo "manifest.json not at zip root" >&2; exit 1; }
echo "$ZIP"
