#!/usr/bin/env bash
# Generates the meetrec icon at multiple sizes under assets/.
# Pois B&W bullseye — outer black disc (96% canvas) + inner white "pois" (44%).
# Requires ImageMagick.
#
# Why several sizes:
# - cargo-bundle builds the macOS .icns from sizes 16,32,64,128,256,512,1024
# - linuxdeploy refuses anything outside its 8..512 whitelist for AppImages
# - runtime window icon uses assets/icon.png (canonical 512x512 copy)
set -euo pipefail

assets="${1:-assets}"
mkdir -p "$assets"

for size in 16 32 64 128 256 512; do
    out="$assets/icon-${size}.png"
    # outer disc r=0.48 * size, inner disc r=0.22 * size, centred
    outer_r=$(( size * 48 / 100 ))
    inner_r=$(( size * 22 / 100 ))
    centre=$(( size / 2 ))
    magick -size "${size}x${size}" xc:transparent \
        -fill "#000000" -draw "circle ${centre},${centre} ${centre},$(( centre - outer_r ))" \
        -fill "#FFFFFF" -draw "circle ${centre},${centre} ${centre},$(( centre - inner_r ))" \
        "$out"
    echo "wrote $out (${size}x${size})"
done

cp "$assets/icon-512.png" "$assets/icon.png"
echo "wrote $assets/icon.png (canonical, 512x512)"
