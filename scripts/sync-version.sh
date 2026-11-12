#!/usr/bin/env bash
# Set the single project version in every manifest.
# Usage: scripts/sync-version.sh 0.2.0
#
# Touches: Cargo.toml ([workspace.package].version, inherited by all crates),
# apps/stow-app/package.json (+ package-lock.json), extension/manifest.json, and
# apps/stow-app/src-tauri/tauri.conf.json if it still carries a "version" key.
# Uses only perl (present on every macOS and Linux) and npm; no sed -i / jq.
set -euo pipefail

V="${1:?usage: scripts/sync-version.sh <semver, e.g. 0.2.0>}"
[[ "$V" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || { echo "not semver: $V" >&2; exit 1; }
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Chrome Web Store versions must be 1-4 dot-separated integers: drop any pre-release suffix.
EXT_V="${V%%-*}"

# Replace the value of the first top-level "version" key in a JSON file.
set_json_version() {
  local file="$1" value="$2"
  perl -0pi -e 's/("version"\s*:\s*")[^"]*(")/${1}'"$value"'${2}/' "$file"
}

# Cargo workspace version: only the line directly under [workspace.package].
perl -0pi -e 's/(\[workspace\.package\]\s*\nversion\s*=\s*")[^"]+(")/${1}'"$V"'${2}/' "$ROOT/Cargo.toml"

# App manifests.
set_json_version "$ROOT/apps/stow-app/package.json" "$V"
if command -v npm >/dev/null 2>&1 && [ -f "$ROOT/apps/stow-app/package-lock.json" ]; then
  # npm rewrites both version fields in the lock file consistently.
  (cd "$ROOT/apps/stow-app" && npm version --no-git-tag-version --allow-same-version "$V" >/dev/null)
fi
if grep -q '"version"' "$ROOT/apps/stow-app/src-tauri/tauri.conf.json"; then
  set_json_version "$ROOT/apps/stow-app/src-tauri/tauri.conf.json" "$V"
fi

# Extension manifest ("manifest_version" is not matched: the regex needs the opening quote).
set_json_version "$ROOT/extension/manifest.json" "$EXT_V"

# Refresh Cargo.lock so the workspace crates carry the new version.
if command -v cargo >/dev/null 2>&1; then
  (cd "$ROOT" && (cargo update --workspace --offline >/dev/null 2>&1 || cargo update --workspace >/dev/null))
fi

echo "version set to $V (extension: $EXT_V)"
"$ROOT/scripts/check-version-sync.sh" "$V"
