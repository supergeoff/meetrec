#!/usr/bin/env bash
# Generates a placeholder 1024x1024 PNG at assets/icon.png.
# Requires ImageMagick.
set -euo pipefail
out="${1:-assets/icon.png}"
mkdir -p "$(dirname "$out")"
magick -size 1024x1024 xc:transparent \
    -fill "#23262c" -draw "circle 512,512 512,32" \
    -fill "#e74c3c" -draw "circle 512,512 512,230" \
    "$out"
echo "Wrote $out"
