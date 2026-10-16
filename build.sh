#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR/wol-highlights"
rm -f "$DIR/wol-highlights.xpi"
zip -r "$DIR/wol-highlights.xpi" * -x "test/*"

echo "Package created at: $DIR/wol-highlights.xpi"
