# Wait until the Windows SDK can actually link, then run the release build.
#
# Why a gate instead of "the installer looks finished": `vcvars64.bat` appearing
# and `link.exe` being on disk are both true well before the SDK's import
# libraries land, and a build started in that window fails at link time with
# nothing to show for it. The check below is the same one that decides success:
# the specific libraries this crate's dependency tree imports.
#
# It waits on the libraries, not on a process list - a process list told us the
# installer was gone when it was still working.

param(
  [int]$TimeoutMinutes = 40,
  [string]$Log = "$PSScriptRoot\..\.msvc-wait-build.log",
  [string]$TargetDir = "E:\target-cache\aimyria-msvc-release"
)

$ErrorActionPreference = "Continue"
function Say($m) {
  $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m
  Write-Output $line
  Add-Content -LiteralPath $Log -Value $line -Encoding utf8
}

$sdkLib = "C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\um\x64"
$sdkInc = "C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\um\windows.h"
# The set is every SDK import lib this dependency tree reaches. A partial SDK
# passes on kernel32 alone, so kernel32 alone is not a sufficient gate.
$needed = @("kernel32.lib", "user32.lib", "advapi32.lib", "shell32.lib", "gdi32.lib",
            "ole32.lib", "oleaut32.lib", "comctl32.lib", "dwmapi.lib", "crypt32.lib",
            "secur32.lib", "ws2_32.lib", "bcrypt.lib", "shlwapi.lib", "uuid.lib")

Set-Content -LiteralPath $Log -Value "" -Encoding utf8
Say "waiting for a linkable Windows SDK"
Say ("lib dir: " + $sdkLib)
$deadline = (Get-Date).AddMinutes($TimeoutMinutes)
$ready = $false
while ((Get-Date) -lt $deadline) {
  $missing = $needed | Where-Object { -not (Test-Path (Join-Path $sdkLib $_)) }
  $inc = Test-Path $sdkInc
  $procs = @(Get-Process -Name setup,vs_setup_bootstrapper,vs_installershell -ErrorAction SilentlyContinue)
  $pids = ($procs | ForEach-Object { "$($_.ProcessName):$($_.Id)" }) -join " "
  if ($missing.Count -eq 0 -and $inc) {
    Say "SDK linkable - all $($needed.Count) libs present and windows.h present"
    $ready = $true
    break
  }
  if ($procs.Count -eq 0) {
    Say ("installer processes gone but SDK incomplete - missing: " + ($missing -join ", ") + "; windows.h=" + $inc)
    Say "RESULT=FAIL sdk-incomplete-and-installer-exited"
    exit 2
  }
  Say ("waiting: missing " + $missing.Count + "/" + $needed.Count + " libs, windows.h=" + $inc + "  installers: " + $pids)
  Start-Sleep -Seconds 20
}

if (-not $ready) { Say "RESULT=FAIL timeout"; exit 3 }
Say "RESULT=SDK_READY"

# -- build -------------------------------------------------------------------
$repo = Split-Path -Parent $PSScriptRoot
Say "starting build: scripts\build_msvc.ps1 -TargetDir $TargetDir"
Say ("ManifestDir " + (Join-Path $repo "src-tauri"))
$sw = [Diagnostics.Stopwatch]::StartNew()
$out = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "build_msvc.ps1") `
        -TargetDir $TargetDir `
        -ManifestDir (Join-Path $repo "src-tauri") `
        -Vcvars "D:\VSBuildTools\VC\Auxiliary\Build\vcvars64.bat" `
        -RustupHome (Join-Path $env:USERPROFILE ".rustup") `
        -CargoHome (Join-Path $env:USERPROFILE ".cargo") 2>&1 | Out-String
$code = $LASTEXITCODE
$sw.Stop()
Say ("build exit $code after {0:N1} min" -f $sw.Elapsed.TotalMinutes)
Add-Content -LiteralPath $Log -Value $out -Encoding utf8
($out -split "`r?`n") | ForEach-Object { Write-Output $_ }
if ($code -ne 0) { Say "RESULT=FAIL build-exit-$code"; exit $code }
Say "RESULT=BUILD_OK"
