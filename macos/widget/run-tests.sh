#!/usr/bin/env bash
# Hermetic widget model, publisher, packaging, and extension compilation checks.
set -euo pipefail
WIDGET_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/cache"

"$WIDGET_DIR/run-model-tests.sh"
python3 -m unittest discover -s "$WIDGET_DIR" -p 'test_build.py'

swiftc -module-cache-path "$STAGE/cache" -O -parse-as-library \
  -target "$(uname -m)-apple-macosx14.0" \
  -D SWIFT_TEST_HARNESS -D WIDGET_HOST \
  "$WIDGET_DIR/../ai-usagebar-menubar.swift" \
  "$WIDGET_DIR/WidgetSnapshot.swift" "$WIDGET_DIR/WidgetPublisher.swift" \
  "$WIDGET_DIR/WidgetPublisherTests.swift" -o "$STAGE/widget-publisher-tests"
"$STAGE/widget-publisher-tests"

swiftc -module-cache-path "$STAGE/cache" -O -parse-as-library \
  -target "$(uname -m)-apple-macosx14.0" \
  -application-extension -D WIDGET_EXTENSION \
  -Xlinker -e -Xlinker _NSExtensionMain \
  "$WIDGET_DIR/WidgetSnapshot.swift" "$WIDGET_DIR/UsageWidgetView.swift" \
  "$WIDGET_DIR/UsageWidget.swift" -o "$STAGE/widget-extension"
python3 - "$WIDGET_DIR" "$STAGE/widget-extension" <<'PY'
from pathlib import Path
import sys
sys.path.insert(0, sys.argv[1])
import build
build.verify_extension_entry_point(Path(sys.argv[2]))
PY
echo "WidgetKit extension compilation and bootstrap entry point passed"
