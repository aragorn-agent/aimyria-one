# Generate Aimyria's icon set.
#
# Upstream's icon is theirs: orange and teal on black. Neither colour is in the
# Aimyria palette, so the mark is redrawn here rather than recoloured -- a
# recolour would keep upstream's silhouette, which is their design, not ours.
#
# The mark: a warm paper tile with a terracotta chevron pointing right, and an
# ink dot at its tip. One direction, one destination -- which is the product.
# It has to stay legible at 16px in the tray, so the shapes are large and there
# is no gradient, no thin stroke, and no transparency in the tile itself.
#
# Run from the repo root:
#   powershell -File tools/make_icons.ps1

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$iconDir = Join-Path $root 'src-tauri\icons'
$trayDir = Join-Path $iconDir 'tray\windows'
New-Item -ItemType Directory -Force -Path $trayDir | Out-Null

# Aimyria palette.
$paper   = [System.Drawing.Color]::FromArgb(255, 0xF2, 0xF1, 0xEC)
$ink     = [System.Drawing.Color]::FromArgb(255, 0x14, 0x14, 0x13)
$brand   = [System.Drawing.Color]::FromArgb(255, 0xAB, 0x5A, 0x30)
$transparent = [System.Drawing.Color]::FromArgb(0, 0, 0, 0)

function New-Tile([int]$size, [switch]$ClearTile) {
    $bmp = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.Clear($transparent)

    if (-not $ClearTile) {
        # Rounded tile. The radius is proportional so the corner reads the same
        # at 32px and at 512px instead of turning into a circle at the top end.
        $r = [Math]::Max(2, [int]($size * 0.22))
        $path = [System.Drawing.Drawing2D.GraphicsPath]::new()
        $d = $r * 2
        $path.AddArc(0, 0, $d, $d, 180, 90)
        $path.AddArc($size - $d, 0, $d, $d, 270, 90)
        $path.AddArc($size - $d, $size - $d, $d, $d, 0, 90)
        $path.AddArc(0, $size - $d, $d, $d, 90, 90)
        $path.CloseFigure()
        $brush = [System.Drawing.SolidBrush]::new($paper)
        $g.FillPath($brush, $path)
        $brush.Dispose()
        $path.Dispose()
    }

    # Chevron: a thick right-pointing angle, inset from the tile.
    $inset = $size * 0.30
    $arm   = $size - $inset * 2
    $pts = @(
        ([System.Drawing.PointF]::new([float]$inset, [float]$inset)),
        ([System.Drawing.PointF]::new([float]($inset + $arm), [float]($size / 2))),
        ([System.Drawing.PointF]::new([float]$inset, [float]($size - $inset)))
    )
    $chevron = [System.Drawing.SolidBrush]::new($brand)
    $g.FillPolygon($chevron, [System.Drawing.PointF[]]$pts)
    $chevron.Dispose()

    # Ink dot at the tip: the one destination everything is pointed at.
    $dotR = [Math]::Max(1.0, [double]$size * 0.085)
    $cx = [float]($size - $size * 0.20)
    $cy = [float]($size / 2)
    $dot = [System.Drawing.SolidBrush]::new($ink)
    $g.FillEllipse($dot, $cx - $dotR, $cy - $dotR, $dotR * 2, $dotR * 2)
    $dot.Dispose()

    $g.Dispose()
    return $bmp
}

function Save-Png([System.Drawing.Bitmap]$bmp, [string]$path, [int]$size) {
    $dst = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($dst)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.Clear($transparent)
    $g.DrawImage($bmp, 0, 0, $size, $size)
    $g.Dispose()
    $dst.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $dst.Dispose()
}

# App icons. Drawn once at 512 and downsampled, so every size gets the same
# antialiasing rather than each one re-rasterising the geometry at its own
# resolution.
$master = New-Tile 512
foreach ($s in 32, 128, 256, 512) {
    Save-Png $master (Join-Path $iconDir "$s`x$s.png") $s
}
Save-Png $master (Join-Path $iconDir '128x128@2x.png') 256
Save-Png $master (Join-Path $iconDir 'icon.png') 512
$master.Dispose()

# .ico. System.Drawing cannot write .ico, and the format is simple enough to
# emit: a six-byte header, then one sixteen-byte directory entry per PNG
# payload.
$sizes = 32, 128, 256
$payloads = @()
foreach ($s in $sizes) {
    $bmp = New-Tile $s
    $ms = [System.IO.MemoryStream]::new()
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $payloads += , $ms.ToArray()
    $bmp.Dispose()
    $ms.Dispose()
}

$ico = [System.IO.MemoryStream]::new()
$w = [System.IO.BinaryWriter]::new($ico)
$w.Write([UInt16]0)          # reserved
$w.Write([UInt16]1)          # type: icon
$w.Write([UInt16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]
    # 256 is encoded as 0 in the single-byte dimension fields.
    $dim = $s
    if ($s -ge 256) { $dim = 0 }
    $w.Write([Byte]$dim)
    $w.Write([Byte]$dim)
    $w.Write([Byte]0)        # palette size
    $w.Write([Byte]0)        # reserved
    $w.Write([UInt16]1)      # colour planes
    $w.Write([UInt16]32)     # bits per pixel
    $w.Write([UInt32]$payloads[$i].Length)
    $w.Write([UInt32]$offset)
    $offset += $payloads[$i].Length
}
foreach ($p in $payloads) { $w.Write($p) }
$w.Flush()
[System.IO.File]::WriteAllBytes((Join-Path $iconDir 'icon.ico'), $ico.ToArray())
$w.Dispose()
$ico.Dispose()

# Tray icons. Windows tray: a transparent tile, because the taskbar background
# varies and an opaque square reads as a glitch.
$trayMaster = New-Tile 256 -ClearTile
foreach ($s in 16, 32, 48) {
    Save-Png $trayMaster (Join-Path $trayDir "tray-$s.png") $s
}
$trayMaster.Dispose()

Write-Output 'icons written:'
Get-ChildItem -LiteralPath $iconDir -Filter '*.png' | ForEach-Object {
    '  {0}  {1} KB' -f $_.Name, [Math]::Round($_.Length / 1KB, 1)
}
Get-ChildItem -LiteralPath $iconDir -Filter '*.ico' | ForEach-Object {
    '  {0}  {1} KB' -f $_.Name, [Math]::Round($_.Length / 1KB, 1)
}
Get-ChildItem -LiteralPath $trayDir -Filter '*.png' | ForEach-Object {
    '  tray/{0}  {1} KB' -f $_.Name, [Math]::Round($_.Length / 1KB, 1)
}