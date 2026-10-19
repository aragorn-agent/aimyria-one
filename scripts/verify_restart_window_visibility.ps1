# Does the main window become visible after the app RESTARTS itself?
#
# WHERE THIS CAME FROM
#
# Not a hunch. A BOM-tolerance run left this in the app's own log, twice:
#
#   [00:18:13] 静默启动模式：主窗口已隐藏      <- the app says it hid the window
#   [00:18:15] [updater] holding the replacement: pending_deeplink=false window_visible=true
#   [00:18:36] [updater] holding the replacement: pending_deeplink=false window_visible=true
#
# `silent_startup` was on (so the window was hidden at startup), nothing was
# focused or brought forward, no deep link was passed -- and yet the updater's own
# guard measured the window as VISIBLE. The owner's complaint is "窗口在运行的时候
# 一直闪几个" (windows flashing repeatedly while it runs). An app that replaces
# itself and restarts, with each restart putting a window on screen, would look
# exactly like that.
#
# WHAT THIS PROBE MEASURES
#
# It runs the exe against a local manifest that advertises a newer version, so the
# updater really performs the replace-and-restart dance, and it samples the
# visibility of *whichever process is currently running from that path* -- across
# the restart boundary, not just the first process. The first process is expected
# to stay hidden; the question is what the successor does.
#
# HONEST SCOPE
#
# `IsWindowVisible` on `class='Tauri Window'` per process. It reports the timeline
# and the pid changes; it does not, by itself, explain the cause.
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [int]$Port = 8811,
  [int]$Newer = 0,
  [int]$Seconds = 60,
  [string]$Work = ""
)

$ErrorActionPreference = "Continue"
if (-not $Work) { $Work = Join-Path $env:TEMP "aimyria-restart-vis" }

Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class VisQ {
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern int GetWindowLong(IntPtr h, int i);
    public static string MainVisible(uint target) {
        string r = "no-window";
        EnumWindows((h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (pid != target) return true;
            var sb = new StringBuilder(256); GetClassName(h, sb, 256);
            if (sb.ToString() == "Tauri Window") {
                r = IsWindowVisible(h) ? "VISIBLE" : "hidden";
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return r;
    }
}
'@ -ErrorAction SilentlyContinue

# The exe's own version, read from its startup banner string.
$bytes = [System.IO.File]::ReadAllBytes($Exe)
$ascii = [System.Text.Encoding]::ASCII.GetString($bytes)
$current = $null
foreach ($v in @("1.0.9","1.0.8","1.0.7","1.0.6","1.0.5","1.0.4","1.0.3","1.0.2","1.0.1","1.0.0")) {
  if ($ascii.Contains("AimyriA go v$v started")) { $current = $v; break }
}
if (-not $current) { Write-Output "cannot read the exe's version"; exit 2 }
$advertise = if ($Newer -gt 0) { $Newer.ToString() } else {
  $p = ([version]$current); "$($p.Major).$($p.Minor).$($p.Build + 1)"
}
Write-Output ("exe       : " + $Exe)
Write-Output ("version   : v$current  ->  manifest advertises v$advertise")

# A work copy: the app replaces whatever path it is running from, and this must not
# be the deliverable in release\ -- an earlier run of a sibling script replaced that
# file six times while the release was being prepared.
if (Test-Path -LiteralPath $Work) { Remove-Item -LiteralPath $Work -Recurse -Force }
$runDir = Join-Path $Work "run"
$serveDir = Join-Path $Work "serve\releases\latest\download"
New-Item -ItemType Directory -Force -Path $runDir, $serveDir | Out-Null
$exeName = "AimyriaOne-x64.exe"
$runExe = Join-Path $runDir $exeName
Copy-Item -LiteralPath $Exe $runExe -Force

# A payload with the advertised version. Building a real 1.0.2 exe is not the point
# of this probe -- the restart behaviour is -- so the payload is a second copy.
Copy-Item -LiteralPath $Exe (Join-Path $serveDir $exeName) -Force
$sha = (Get-FileHash -LiteralPath $Exe -Algorithm SHA256).Hash.ToLower()
$json = [ordered]@{
  version = $advertise
  url     = "http://127.0.0.1:$Port/releases/latest/download/$exeName"
  sha256  = $sha
  notes   = "restart visibility probe"
  pubDate = (Get-Date).ToString("yyyy-MM-ddTHH:mm:sszzz")
} | ConvertTo-Json -Compress
[System.IO.File]::WriteAllBytes((Join-Path $serveDir "version.json"),
  [System.Text.Encoding]::UTF8.GetBytes($json))

$serverScript = Join-Path (Split-Path -Parent $PSScriptRoot) "scripts\serve_updater_release.mjs"
$job = Start-Job -ScriptBlock { param($s,$d,$p) & node $s $d $p 2>&1 } -ArgumentList $serverScript, (Join-Path $Work "serve"), $Port
$url = "http://127.0.0.1:$Port/releases/latest/download/version.json"
for ($i = 0; $i -lt 60; $i++) {
  try { Invoke-WebRequest -Uri $url -TimeoutSec 3 -UseBasicParsing -ErrorAction Stop | Out-Null; break }
  catch { Start-Sleep -Milliseconds 250 }
}
Write-Output ("serving   : " + $url)
Write-Output ""

$home_ = Join-Path $Work "home"
New-Item -ItemType Directory -Force -Path $home_ | Out-Null
$env:CC_SWITCH_TEST_HOME = $home_
$env:HOME = $home_
$env:AIMYRIA_UPDATE_TEST_CLOCK = "1"
$env:AIMYRIA_UPDATE_MANIFEST_URL = $url
Remove-Item Env:\AIMYRIA_UPDATE_DRYRUN -ErrorAction SilentlyContinue
try { $proc = Start-Process -FilePath $runExe -PassThru -ErrorAction Stop }
catch { Write-Output ("[FAIL] could not launch: " + $_.Exception.Message); exit 2 }
finally { Remove-Item Env:\AIMYRIA_UPDATE_TEST_CLOCK, Env:\AIMYRIA_UPDATE_MANIFEST_URL, Env:\CC_SWITCH_TEST_HOME, Env:\HOME -ErrorAction SilentlyContinue }

$t0 = Get-Date
Write-Output ("launched  : pid " + $proc.Id + " at " + $t0.ToString("HH:mm:ss.fff"))
Write-Output "samples (only changes are printed):"
$lastPid = $proc.Id
$lastState = $null
$everVisiblePid = @{}
$deadline = $t0.AddSeconds($Seconds)
while ((Get-Date) -lt $deadline) {
  # Whoever is running from this path right now: after a restart it is a new pid.
  $live = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $runExe })
  if ($live.Count -eq 0) { Start-Sleep -Milliseconds 150; continue }
  $p = $live[0]
  $state = [VisQ]::MainVisible([uint32]$p.Id)
  if ($p.Id -ne $lastPid) {
    Write-Output ("  +$([Math]::Round(((Get-Date)-$t0).TotalSeconds,2))s  NEW PID " + $p.Id + " (was " + $lastPid + ")  window=" + $state)
    $lastPid = $p.Id; $lastState = $state
  } elseif ($state -ne $lastState) {
    Write-Output ("  +$([Math]::Round(((Get-Date)-$t0).TotalSeconds,2))s  pid " + $p.Id + "  window " + $lastState + " -> " + $state)
    $lastState = $state
  }
  if ($state -eq "VISIBLE") { $everVisiblePid[$p.Id] = $true }
  Start-Sleep -Milliseconds 120
}

Write-Output ""
Write-Output "=== verdict ==="
Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $runExe } | ForEach-Object { Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue }
Stop-Job -Job $job -ErrorAction SilentlyContinue; Remove-Job -Job $job -Force -ErrorAction SilentlyContinue

if ($everVisiblePid.Count -eq 0) {
  Write-Output "  [PASS] no process running from this path ever had a visible main window"
} else {
  Write-Output ("  [FAIL] " + $everVisiblePid.Count + " pid(s) had a VISIBLE main window: " + (($everVisiblePid.Keys) -join ", "))
}
Write-Output ("  tree: " + $Work)
