# Draw the Aimyria AA monogram as the app's window + taskbar icon.
#
# Replaces `src-tauri/icons/*.png` and `icon.ico`, which were upstream's — a
# stylised wave with nothing to do with a relay, on a product called Aimyria.
#
# The mark is two congruent "A" forms, the second rotated a half turn so the pair
# locks apex-to-apex and shares the first one's crossbar. It reads as a single
# glyph at 16 px and as two letters at 256.
#
# Geometry is drawn with GDI+ rather than rasterising SVG: exact at every size, no
# external toolchain, and the same three primitives as
# src/components/brand/AimyriaWordmark.tsx. Stroked, not filled — a stroked "A"
# keeps its weight down at 16 px where a filled one becomes a blob; round caps and
# joins are what stop it reading as a warning triangle there.

$dir = "D:\src\cc-switch-main\src-tauri\icons"
$sizes = @(16, 20, 24, 32, 40, 48, 64, 128, 256)

Add-Type -AssemblyName System.Drawing

foreach ($px in $sizes) {
  $bmp = New-Object System.Drawing.Bitmap([int]$px, [int]$px)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $g.Clear([System.Drawing.Color]::Transparent)

  $s = [double]$px / 32.0
  $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 217, 119, 90), [Math]::Max(1.0, 2.6 * $s))
  $pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
  $pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
  $pen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round

  # One "A" in the 32-unit box: apex (16,8) to feet (8.6,24) and (23.4,24),
  # crossbar at y=18.6.
  $draw = {
    param($x1, $y1, $x2, $y2)
    $g.DrawLine($pen, [float]($x1 * $s), [float]($y1 * $s), [float]($x2 * $s), [float]($y2 * $s))
  }

  # Left A, apex up.
  & $draw 16 8 8.6 24
  & $draw 16 8 23.4 24
  & $draw 12.1 18.6 19.9 18.6

  # Right A, the same glyph apex-down.
  $half = [double]$px / 2.0
  $g.TranslateTransform([float]$half, [float]$half)
  $g.RotateTransform([float]180)
  $g.TranslateTransform([float](-$half), [float](-$half))
  & $draw 16 8 8.6 24
  & $draw 16 8 23.4 24

  $g.Dispose()
  $name = if ($px -eq 128) { "128x128.png" } else { "$px" + "x" + "$px" + ".png" }
  $bmp.Save((Join-Path $dir $name), [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  Write-Output ("  " + $name)
}

# ---- icon.ico ----
# ICO entries carry PNG-compressed data, which is what Tauri bundles for the window
# icon, so the PNG bytes go in verbatim rather than re-encoded.
$icoPath = Join-Path $dir "icon.ico"
$fs = [System.IO.File]::Create($icoPath)
$bw = New-Object System.IO.BinaryWriter($fs)

$bw.Write([uint16]0)      # reserved
$bw.Write([uint16]1)      # type: icon
$bw.Write([uint16]$sizes.Count)

$payload = @()
foreach ($px in $sizes) {
  $name = if ($px -eq 128) { "128x128.png" } else { "$px" + "x" + "$px" + ".png" }
  $payload += ,@((Join-Path $dir $name), $px)
}

$offset = 6 + (16 * $sizes.Count)
foreach ($p in $payload) {
  $bytes = [System.IO.File]::ReadAllBytes($p[0])
  $w = if ($p[1] -ge 256) { 0 } else { [int]$p[1] }
  $bw.Write([byte]$w)
  $bw.Write([byte]$w)
  $bw.Write([byte]0)        # palette count: 0 = PNG
  $bw.Write([byte]0)
  $bw.Write([uint16]1)      # planes
  $bw.Write([uint16]32)     # bpp
  $bw.Write([uint32]$bytes.Length)
  $bw.Write([uint32]$offset)
  $offset = $offset + $bytes.Length
}
foreach ($p in $payload) {
  $bw.Write([System.IO.File]::ReadAllBytes($p[0]))
}
$bw.Flush()
$bw.Close()
$fs.Close()
Write-Output "  icon.ico"

Write-Output ""
Write-Output ("wrote " + $sizes.Count + " sizes + icon.ico to " + $dir)
