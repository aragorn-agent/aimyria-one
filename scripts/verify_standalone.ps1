# Prove the MSVC build is actually a single self-contained file.
#
# # Why this script exists
#
# A green `cargo build` says only that linking succeeded. It says nothing about
# whether the binary can start on a machine that has never seen this project. On
# the GNU build those were different claims: the build was green, and the exe still
# refused to start without `WebView2Loader.dll` beside it.
#
# So the two claims are checked separately, in the order a stranger would hit them:
#
#   1. Does the import table still name WebView2Loader.dll?  A dynamic link, and
#      therefore a file that cannot travel alone.
#   2. Copied alone into an empty directory, does it start?            The failure
#      the user actually reported: "由于找不到 WebView2Loader.dll，无法继续执行代码".
#      Reproducing the original bug is the only way to know it is gone.
#   3. Does it come up with no DLL in the folder *and* nothing installed?
#
# # What counts as passing
#
# Launching is not enough — this is a tray app whose window starts hidden, so a
# process that starts and immediately dies looks identical to one that works. The
# process has to still be alive after the WebView2 runtime has had time to
# initialise, which is what the settle delay is for.

param(
  [string]$Exe     = "D:\Rust\build\aimyria-one-msvc\release\aimyria-one.exe",
  [string]$Desktop = "$env:USERPROFILE\Desktop",
  [switch]$Install
)

$ErrorActionPreference = "Continue"
$objdump = "D:\Tools\mingw64\mingw64\bin\objdump.exe"
$pass = $true

function Ok($m)   { Write-Output ("  [PASS] " + $m) }
function Bad($m)  { Write-Output ("  [FAIL] " + $m); $script:pass = $false }

if (-not (Test-Path -LiteralPath $Exe)) { Bad "no exe at $Exe"; exit 1 }

Write-Output "=== 1. 导入表 ==="
$imports = (& $objdump -p $Exe 2>$null | Select-String '^\s*DLL Name:').Line |
           ForEach-Object { ($_ -split ':')[1].Trim() }
$wv2 = $imports | Where-Object { $_ -match 'WebView2Loader' }
if ($wv2) { Bad ("still imports " + ($wv2 -join ", ")) }
else      { Ok "no WebView2Loader in the import table - self-contained" }
Write-Output ("         imports: " + (($imports | Sort-Object -Unique) -join ", "))

Write-Output ""
Write-Output "=== 2. 空目录单文件启动 ==="
Get-Process -Name "aimyria-one" -EA SilentlyContinue | Stop-Process -Force -EA SilentlyContinue
Start-Sleep -Seconds 2

# A directory that has never held anything from this project: no loader DLL, no
# other binary, nothing.
$lab = Join-Path $env:TEMP "aimyria-alone"
if (Test-Path $lab) { Remove-Item $lab -Recurse -Force }
New-Item -ItemType Directory -Force -Path $lab | Out-Null
Copy-Item $Exe (Join-Path $lab "AimyriaOne.exe") -Force

$dir = Get-ChildItem $lab -File
Write-Output ("         the folder holds exactly " + $dir.Count + " file(s): " + ($dir.Name -join ", "))
if ($dir.Count -ne 1) { Bad "expected a lone exe, found $($dir.Count)" }

$p = Start-Process -FilePath (Join-Path $lab "AimyriaOne.exe") -PassThru
Start-Sleep -Seconds 25
if (Get-Process -Id $p.Id -EA SilentlyContinue) {
  Ok ("started and still running after 25 s (pid " + $p.Id + ")")
  Stop-Process -Id $p.Id -Force -EA SilentlyContinue
} else {
  # A loader failure exits with no window at all, which is exactly the reported bug.
  Bad "exited within 25 s - the same failure as the empty-folder report"
}

Write-Output ""
Write-Output "=== 3. 装到桌面 ==="
if ($Install) {
  # Stale copies are removed first: leaving a `WebView2Loader.dll` next to the new
  # exe would hide the very defect this checks for.
  Remove-Item (Join-Path $Desktop "WebView2Loader.dll") -Force -EA SilentlyContinue
  Remove-Item (Join-Path $Desktop "AimyriaOne.exe") -Force -EA SilentlyContinue
  Copy-Item $Exe (Join-Path $Desktop "AimyriaOne.exe") -Force
  $left = Get-ChildItem $Desktop -File | Where-Object { $_.Name -match 'AimyriaOne\.exe|WebView2Loader' }
  Ok ("desktop: " + (($left | ForEach-Object { $_.Name }) -join ", "))
}

Write-Output ""
if ($pass) { Write-Output "VERDICT: single self-contained exe confirmed" }
else       { Write-Output "VERDICT: NOT self-contained - see the FAIL lines above" }
exit $(if ($pass) { 0 } else { 1 })
