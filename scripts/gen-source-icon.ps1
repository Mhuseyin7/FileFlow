# Generate a 1024x1024 source PNG for FileFlow icons. Writes
# src-tauri/icons/_source.png. Downstream, `npx tauri icon` fans this out
# into every platform-specific size the bundler needs.
param(
    [string]$OutDir = (Join-Path (Split-Path -Parent $PSScriptRoot) 'src-tauri/icons')
)

Add-Type -AssemblyName System.Drawing

$size = 1024
$bmp  = New-Object System.Drawing.Bitmap($size, $size)
$g    = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode     = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAlias

$bg   = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(79, 140, 255))
$path = New-Object System.Drawing.Drawing2D.GraphicsPath
$r    = 180
$path.AddArc(0,          0,          $r*2, $r*2, 180, 90)
$path.AddArc($size-$r*2, 0,          $r*2, $r*2, 270, 90)
$path.AddArc($size-$r*2, $size-$r*2, $r*2, $r*2, 0,   90)
$path.AddArc(0,          $size-$r*2, $r*2, $r*2, 90,  90)
$path.CloseFigure()
$g.FillPath($bg, $path)

$white = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::White)
$font  = New-Object System.Drawing.Font('Segoe UI', 640, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
$sf    = New-Object System.Drawing.StringFormat
$sf.Alignment     = [System.Drawing.StringAlignment]::Center
$sf.LineAlignment = [System.Drawing.StringAlignment]::Center
$rect  = New-Object System.Drawing.RectangleF 0, -40, $size, $size
$g.DrawString('F', $font, $white, $rect, $sf)

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$out = Join-Path $OutDir '_source.png'
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)

$g.Dispose(); $bmp.Dispose()
Write-Host "Wrote $out"
