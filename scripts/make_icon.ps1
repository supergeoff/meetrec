# Generates the meetrec icon at multiple sizes under assets/.
# Pois B&W bullseye — outer black disc (96% canvas) + inner white disc (44%).
# Matches design-extracted/.../meetrec-mark.svg.
#
# Why several sizes:
# - cargo-bundle builds the macOS .icns from sizes 16,32,64,128,256,512,1024
# - linuxdeploy refuses anything outside its 8..512 whitelist for AppImages
# - runtime window icon uses assets/icon.png (512 is the canonical copy)

param(
    [string]$AssetsDir = "assets"
)

Add-Type -AssemblyName System.Drawing

# Targeted sizes. Keep 512 last so it ends up copied to `icon.png` below.
$sizes = @(16, 32, 64, 128, 256, 512)

function New-MeetrecIcon([int]$size, [string]$path) {
    $bitmap = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.Clear([System.Drawing.Color]::Transparent)

    $blackBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 0, 0, 0))
    $outerSize = [int]($size * 0.96)
    $outerOffset = [int](($size - $outerSize) / 2)
    $graphics.FillEllipse($blackBrush, $outerOffset, $outerOffset, $outerSize, $outerSize)
    $blackBrush.Dispose()

    $whiteBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 255, 255, 255))
    $innerSize = [int]($size * 0.44)
    $innerOffset = [int](($size - $innerSize) / 2)
    $graphics.FillEllipse($whiteBrush, $innerOffset, $innerOffset, $innerSize, $innerSize)
    $whiteBrush.Dispose()

    $graphics.Dispose()
    $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bitmap.Dispose()
}

if (-not (Test-Path $AssetsDir)) {
    New-Item -ItemType Directory -Path $AssetsDir | Out-Null
}

foreach ($s in $sizes) {
    $out = Join-Path $AssetsDir "icon-${s}.png"
    New-MeetrecIcon -size $s -path $out
    Write-Host "wrote $out (${s}x${s})"
}

# Canonical copy used by the runtime window icon (include_bytes!) and by the
# Linux AppImage step.
Copy-Item -Force (Join-Path $AssetsDir "icon-512.png") (Join-Path $AssetsDir "icon.png")
Write-Host "wrote $(Join-Path $AssetsDir 'icon.png') (canonical, 512x512)"
