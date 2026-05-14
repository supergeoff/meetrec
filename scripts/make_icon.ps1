# Generates the meetrec icon at assets/icon.png (1024x1024).
# Pois B&W bullseye — see .design-extracted/.../meetrec-mark.svg
#   outer:  100% black circle, radius ~ 0.48 of canvas
#   inner:  white circle (the "pois" hole), radius ~ 0.22
param(
    [string]$OutPath = "assets/icon.png"
)

Add-Type -AssemblyName System.Drawing

$size = 1024
$bitmap = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$graphics.Clear([System.Drawing.Color]::Transparent)

# Outer black disc
$blackBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 0, 0, 0))
$outerSize = [int]($size * 0.96)
$outerOffset = [int](($size - $outerSize) / 2)
$graphics.FillEllipse($blackBrush, $outerOffset, $outerOffset, $outerSize, $outerSize)
$blackBrush.Dispose()

# Inner white "pois"
$whiteBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 255, 255, 255))
$innerSize = [int]($size * 0.44)
$innerOffset = [int](($size - $innerSize) / 2)
$graphics.FillEllipse($whiteBrush, $innerOffset, $innerOffset, $innerSize, $innerSize)
$whiteBrush.Dispose()

$graphics.Dispose()
$bitmap.Save($OutPath, [System.Drawing.Imaging.ImageFormat]::Png)
$bitmap.Dispose()

Write-Host "Wrote $OutPath ($size x $size, Pois B&W bullseye)"
