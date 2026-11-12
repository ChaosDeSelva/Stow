#!/usr/bin/env bash
# Print the CHANGELOG.md section for one version (Keep a Changelog headings).
# Usage: scripts/changelog-section.sh 0.2.0
set -euo pipefail
V="${1:?usage: scripts/changelog-section.sh <version>}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
awk -v v="$V" '
  /^## \[/ { if (found) exit; if (index($0, "## [" v "]") == 1) { found = 1; next } }
  found { print }
' "$ROOT/CHANGELOG.md" | perl -0pe 's/^\n+//; s/\n+\z/\n/'
