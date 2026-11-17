#!/usr/bin/env bash
# Render the real SwiftUI view using only the labeled synthetic fixture.
set -euo pipefail
if [[ $# -ne 1 || "$1" != /* ]]; then
  echo "Usage: $0 /absolute/output/directory" >&2
  exit 2
fi
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/cache"
swiftc -module-cache-path "$TMP/cache" -parse-as-library \
  -target "$(uname -m)-apple-macosx14.0" \
  "$DIR/WidgetSnapshot.swift" "$DIR/UsageWidgetView.swift" "$DIR/render-preview.swift" \
  -o "$TMP/widget-preview-renderer"
"$TMP/widget-preview-renderer" "$1"
