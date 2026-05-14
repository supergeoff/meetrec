# Generates a placeholder 1024x1024 PNG at assets/icon.png.
# Replace assets/icon.png with your own production artwork before shipping.
param(
    [string]$OutPath = "assets/icon.png"
)

Add-Type -AssemblyName System.Drawing

$size = 1024
$bitmap = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$graphics.Clear([System.Drawing.Color]::Transparent)

# Outer ring
$ringBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 35, 38, 44))
$graphics.FillEllipse($ringBrush, 32, 32, $size - 64, $size - 64)
$ringBrush.Dispose()

# Inner red dot (record symbol)
$dotBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 231, 76, 60))
$inset = 230
$graphics.FillEllipse($dotBrush, $inset, $inset, $size - 2 * $inset, $size - 2 * $inset)
$dotBrush.Dispose()

$graphics.Dispose()
$bitmap.Save($OutPath, [System.Drawing.Imaging.ImageFormat]::Png)
$bitmap.Dispose()

Write-Host "Wrote $OutPath ($size x $size)"
