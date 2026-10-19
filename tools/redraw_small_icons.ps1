# Redraw the small icon sizes.
#
# The double-A monogram works at 512 — two A's apex-to-apex, sharing a crossbar. At
# 16 px it does not: the strokes are 1.3 px, anti-aliasing eats the edges, and the
# pair fuses into a blob that reads as neither a letter nor a mark. The preview
# after the first pass was a smudge.
#
# So the small sizes get a different mark: a single bold A with a heavier stroke,
# which at 16 px is unmistakable. An "A" that reads beats an "AA" that doesn't, and
# the owner's complaint was about the taskbar icon specifically.
#
# Everything else — the palette, the round caps, the geometry's origin — is the
# same, so the two marks still look like one family.

param([string]$Dir)

if (-not $Dir) { $Dir = "D:\src\cc-switch-main\src-tauri\icons" }

Add-Type -AssemblyName System.Drawing

function Save-Mark([int]$px, [switch]$Double, [double]$stroke) {
  $bmp = New-Object System.Drawing.Bitmap([int]$px, [int]$px)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $g.Clear([System.Drawing.Color]::Transparent)

  $s = [double]$px / 32.0
  $pen = New-Object System.Drawing.Pen(
    [System.Drawing.Color]::FromArgb(255, 217, 119, 90),
    [double]($stroke * $s))
  $pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
  $pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
  $pen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round

  $line = {
    param([double]$a, [double]$b, [double]$c, [double]$e)
    $g.DrawLine(
      $pen,
      [float]($a * $s), [float]($b * $s),
      [float]($c * $s), [float]($e * $s))
  }

  if ($Double) {
    # Two A's, the second rotated 180 degrees about the centre.
    & $line 16 8 8.6 24
    & $line 16 8 23.4 24
    & $line 12.1 18.6 19.9 18.6

    $half = [double]$px / 2.0
    $g.TranslateTransform([float]$half, [float]$half)
    $g.RotateTransform([float]180)
    $g.TranslateTransform([float](-$half), [float](-$half))
    & $line 16 8 8.6 24
    & $line 16 8 23.4 24
  } else {
    # One A, drawn heavier: apex (16,7), feet (7.5,25) and (24.5,25), crossbar low.
    & $line 16 7 7.5 25
    & $line 16 7 24.5 25
    & $line 11.4 18 20.6 18
  }

  $g.Dispose()
  $name = "$px" + "x" + "$px" + ".png"
  $bmp.Save((Join-Path -Path $Dir -ChildPath $name),
            [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  Write-Output ("  " + $name)
}

# 16-48: single A, 3.4 weight against anti-aliasing. 64+: the double-A monogram.
foreach ($px in @(16, 20, 24, 32, 40, 48)) {
  Save-Mark $px -stroke 3.4
}

# Rebuild icon.ico with the three sizes Windows actually asks the shell for:
# 16 (taskbar / small icons), 32 (Alt-Tab), 48 (window title bar).
$icoPath = Join-Path -Path $Dir -ChildPath "icon.ico"
$fs = [System.IO.File]::Create($icoPath)
$bw = New-Object System.IO.BinaryWriter($fs)

$bw.Write([uint16]0)          # reserved
$bw.Write([uint16]1)          # type: icon
$bw.Write([uint16]3)          # count

$payload = @()
foreach ($px in @(16, 32, 48)) {
  $bytes = [System.IO.File]::ReadAllBytes(
    (Join-Path -Path $Dir -ChildPath ("$px" + "x" + "$px" + ".png")))
  $payload += ,@($bytes, $px)
}

$offset = 6 + (16 * 3)
foreach ($p in $payload) {
  $bw.Write([byte]$p[1])      # width, 256 encoded as 0
  $bw.Write([byte]$p[1])      # height
  $bw.Write([byte]0)          # palette count, 0 = PNG
  $bw.Write([byte]0)          # reserved
  $bw.Write([uint16]1)        # planes
  $bw.Write([uint16]32)       # bpp
  $bw.Write([uint32]$p[0].Length)
  $bw.Write([uint32]$offset)
  $offset = $offset + $p[0].Length
}
foreach ($p in $payload) {
  $bw.Write($p[0])
}
$bw.Flush()
$bw.Close()
$fs.Close()
Write-Output "  icon.ico"

Write-Output ""
Write-Output ("rewrote 6 PNGs + icon.ico in " + $Dir)
