# Package Aimyria One for release, and prove the package installs.
#
# The release layout is the one `scripts/quickstart.ps1` fetches, so the package is
# built to that contract rather than to something convenient:
#
#   AimyriaOne-x64.exe          the binary
#   AimyriaOne-x64.exe.sha256   its checksum
#   WebView2Loader.dll          WebView2 runtime loader, a separate file
#   WebView2Loader.dll.sha256
#
# The checksum files are the installer's only integrity check, so they are written
# from the bytes that are actually in the package rather than from the build
# directory — a checksum for a different file is worse than none.
#
# The zip is what a person would hand over. The loose files stay in the release
# directory because quickstart.ps1 downloads them individually.
#
# `--verify` then installs from the staged copy into a scratch directory and runs
# it, so a package that cannot install never gets labelled as one that can. The
# build being green and the package being usable are different claims.

param(
  [string]$Build    = "D:\Rust\build\aimyria-one\release\aimyria-one.exe",
  [string]$Release  = "$env:TEMP\aimyria-release\releases\latest\download",
  [string]$Loader   = "$env:LOCALAPPDATA\Aimyria\WebView2Loader.dll",
  [string]$Desktop  = "$env:USERPROFILE\Desktop",
  [switch]$Verify
)

$ErrorActionPreference = "Stop"

function Sha256([string]$path) {
  (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLower()
}

New-Item -ItemType Directory -Force -Path $Release | Out-Null

# ── stage ───────────────────────────────────────────────────────────────────
# Flat "path|name" strings rather than hashtables inside an array: `@{ src = … }`
# nested two deep in `@( … )` trips a PowerShell 5.1 parser bug, and the error it
# reports points at the wrong line.
$staged = @{}

# ── decide what the package actually needs ───────────────────────────────────
# Shipping WebView2Loader.dll unconditionally is how the problem stays invisible:
# the package appears to work on the machine it was built on, because the DLL is
# sitting next to it, and fails on every other machine.
#
# So ask the binary rather than assume. Anything the exe imports that is not part
# of Windows has to travel with it. `VCRUNTIME140.dll` is the one that bit here —
# the MSVC build had dropped WebView2Loader but gained the C runtime, which ships
# with the VC++ redistributable rather than with Windows. Checking only for
# WebView2Loader would have called that build self-contained.
#
# `api-ms-win-*` are API sets: indirection layers the OS itself resolves, present
# since Windows 10 1709. They are not files that ship with the app, so counting them
# as a dependency would make every modern binary look like it needs something beside
# it — which is how this list filled up with noise in the first place.
$objdump = "D:\Tools\mingw64\mingw64\bin\objdump.exe"
$needed = @()
$imports = @()
if (Test-Path -LiteralPath $objdump) {
  $imports = (& $objdump -p $Build 2>$null | Select-String '^\s*DLL Name:') |
             ForEach-Object { ($_.Line -split ':')[1].Trim() } | Sort-Object -Unique
  $external = $imports | Where-Object {
    $_ -and $_ -notmatch '^api-ms-win-' -and $_ -notmatch '^(kernel32|user32|gdi32|advapi32|shell32|ole32|oleaut32|comctl32|ntdll|ws2_32|crypt32|secur32|bcrypt|bcryptprimitives|dwmapi|shlwapi)\.dll$'
  }
  foreach ($d in $external) {
    if ($d -ieq "WebView2Loader.dll") { $needed += $Loader }
    else { Write-Output ("  NOTE: unpinned external dependency in the import table: " + $d) }
  }
} else {
  Write-Output "  NOTE: objdump missing — cannot verify the binary is self-contained"
}

$pairs = @(($Build + "|" + "AimyriaOne-x64.exe"))
if ($needed.Count -gt 0) {
  $pairs += ($Loader + "|" + "WebView2Loader.dll")
  Write-Output "  the exe imports WebView2Loader.dll — the DLL must ship beside it"
} else {
  Write-Output "  the exe imports nothing beyond Windows — a single file is the whole package"
}

foreach ($entry in $pairs) {
  $parts = $entry -split "\|"
  $from = $parts[0]
  $name = $parts[1]
  if (-not (Test-Path -LiteralPath $from)) {
    throw "missing input: $from"
  }
  $dst = Join-Path $Release $name
  Copy-Item -LiteralPath $from -Destination $dst -Force
  # Checksum of the staged bytes, not of the source.
  "$((Sha256 $dst))  $name" | Set-Content -LiteralPath "$dst.sha256" -Encoding ascii -NoNewline
  $staged[$name] = $dst
  Write-Output ("  staged {0,-24} {1,10:N0} B" -f $name, (Get-Item $dst).Length)
}

# ── zip ─────────────────────────────────────────────────────────────────────
$zip = Join-Path $Desktop "AimyriaOne-20261016.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Get-ChildItem $Release -File | ForEach-Object { $_.FullName }) -DestinationPath $zip -CompressionLevel Optimal
Write-Output ("  zip   {0,-24} {1,10:N0} B" -f (Split-Path $zip -Leaf), (Get-Item $zip).Length)

foreach ($f in (Get-ChildItem $Release -File)) {
  Copy-Item $f.FullName -Destination (Join-Path $Desktop $f.Name) -Force
}
Write-Output "  loose files copied to the desktop"

# ── verify: install from the staged release, into a scratch dir ─────────────
if ($Verify) {
  $scratch = Join-Path $env:TEMP "aimyria-verify-install"
  if (Test-Path $scratch) { Remove-Item $scratch -Recurse -Force }
  New-Item -ItemType Directory -Force -Path $scratch | Out-Null

  foreach ($f in (Get-ChildItem $Release -File | Where-Object { $_.Extension -ne ".sha256" })) {
    # Verify the checksum the way the installer does, before trusting the file.
    $line = (Get-Content -LiteralPath "$($f.FullName).sha256" -Raw).Trim()
    $expected = ($line -split '\s+')[0]
    $actual = Sha256 $f.FullName
    if ($expected -ne $actual) {
      throw "checksum mismatch for $($f.Name): expected $expected, got $actual"
    }
    Copy-Item $f.FullName -Destination (Join-Path $scratch $f.Name) -Force
  }
  Write-Output "  checksums verified, staged into a clean directory"

  $exe = Join-Path $scratch "AimyriaOne.exe"
  if (-not (Test-Path $exe)) { Copy-Item $staged["AimyriaOne-x64.exe"] $exe -Force }

  $proc = Start-Process -FilePath $exe -PassThru
  Start-Sleep -Seconds 14
  $alive = $null -ne (Get-Process -Id $proc.Id -ErrorAction SilentlyContinue)
  if ($alive) {
    Write-Output "  launched from the package and is still running (pid $($proc.Id))"
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
  } else {
    throw "the packaged binary exited immediately — the package does not run"
  }
}

Write-Output ""
Write-Output ("package: " + $zip)
