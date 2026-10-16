#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUTPUT="$DIR/wol-highlights.xpi"
SRC_DIR="$DIR/wol-highlights"

echo "Building WOL Highlights extension package..."
rm -f "$OUTPUT"

(
  cd "$SRC_DIR"
  zip -r "$OUTPUT" . -x "test/*" -x "*.DS_Store" -x ".*"
)

echo "Package successfully created at: $OUTPUT"
