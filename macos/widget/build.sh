#!/usr/bin/env bash
# Build a signed containing app and its real WidgetKit extension, without installing either.
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec python3 "$DIR/build.py" "$@"
