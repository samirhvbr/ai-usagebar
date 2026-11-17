#!/usr/bin/env bash
# Pure Foundation tests: no App Group, user preferences, CLI, or live API.
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/cache"
swiftc -module-cache-path "$TMP/cache" -parse-as-library \
  "$DIR/WidgetSnapshot.swift" "$DIR/WidgetSnapshotTests.swift" \
  -o "$TMP/widget-model-tests"
"$TMP/widget-model-tests"
