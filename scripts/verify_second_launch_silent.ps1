# Requirement 7: does a link-less second launch raise the window?
#
# THE DEFECT THIS PROVES
#
# The single-instance callback used to end with an unconditional "Show and focus
# window regardless". Any second launch of the exe triggers that callback, and a
# second launch with no deep link is not a request to see the UI -- it is a script,
# a duplicated autostart entry, a scheduled task, or the updater restarting the app
# after replacing the executable. Each one popped the window to the front and stole
# focus. That is the owner's "打开的时候、运行的时候会一直闪几个窗口".
#
# WHY A PLAIN "START IT AND WATCH" TEST CANNOT SEE THIS
#
# A single clean launch never invokes the callback, so a sampler watching one
# process from startup sees a correctly hidden window and reports PASS. That is
# exactly what happened: two earlier runs measured "no visible window at any
# sample" on a build that had the bug. The trigger is the *second* launch, so the
# test has to perform one.
#
# THE TEST, AND WHY IT IS SELF-VALIDATING
#
#   1. start instance A from its own copy and wait until its window is (correctly)
#      hidden -- a baseline, so a later "visible" cannot be blamed on startup;
#   2. launch instance B from the same path with NO arguments -- this is the
#      link-less second launch under test;
#   3. sample A's main window for a while and assert it never becomes visible.
#
# Step 2 can be satisfied by the OS in two different ways, and the difference
# matters:
#   * B exits immediately and A logs "Single Instance Callback Triggered" -> the
#     callback ran, and A's window must stay hidden (this is the fix under test);
#   * B stays alive as its own process -> it did NOT join A, so the callback never
#     ran and this run proves nothing. That is reported as INCONCLUSIVE, not PASS.
# Without that check the test would pass vacuously whenever single-instance
# forwarding failed -- the same "green while measuring nothing" shape this project
# has hit repeatedly.
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [int]$Seconds = 14,
  [int]$IntervalMs = 80,
  [string]$Work = ""
)

$ErrorActionPreference = "Continue"
$results = [System.Collections.Generic.List[string]]::new()
function Pass($m) { Write-Output ("  [PASS] " + $m); $results.Add("PASS  " + $m) }
function Fail($m) { Write-Output ("  [FAIL] " + $m); $results.Add("FAIL  " + $m) }
function Info($m) { Write-Output ("  " + $m) }

if (-not $Work) { $Work = Join-Path $env:TEMP "aimyria-second-launch" }

Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class Vis2 {
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    public static string State(uint target) {
        string r = "no-window";
        EnumWindows((h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (pid != target) return true;
            var sb = new StringBuilder(256); GetClassName(h, sb, 256);
            if (sb.ToString() == "Tauri Window") { r = IsWindowVisible(h) ? "VISIBLE" : "hidden"; return false; }
            return true;
        }, IntPtr.Zero);
        return r;
    }
    // Whether the main window is the *foreground* window -- i.e. focus was stolen.
    public static bool IsForeground(uint target) {
        IntPtr fg = GetForegroundWindow();
        if (fg == IntPtr.Zero) return false;
        uint pid; GetWindowThreadProcessId(fg, out pid);
        return pid == target;
    }
}
'@ -ErrorAction SilentlyContinue

# The run copy lives here, never in release\: the app replaces whatever path it is
# running from, and an earlier script of this series rewrote the deliverable in
# place while a release was being prepared.
if (Test-Path -LiteralPath $Work) { Remove-Item -LiteralPath $Work -Recurse -Force }
$runDir = Join-Path $Work "run"
New-Item -ItemType Directory -Force -Path $runDir | Out-Null
$exeName = "AimyriaOne-x64.exe"
$runExe = Join-Path $runDir $exeName
Copy-Item -LiteralPath $Exe $runExe -Force

$home_ = Join-Path $Work "home"
New-Item -ItemType Directory -Force -Path $home_ | Out-Null
$logPath = Join-Path $home_ ".aimyria\logs\aimyria-one.log"

function Read-LogBestEffort($path) {
  for ($i = 0; $i -lt 12; $i++) {
    try {
      if (Test-Path -LiteralPath $path) {
        $fs = [System.IO.File]::Open($path, 'Open', 'Read', 'ReadWrite')
        try { $sr = New-Object System.IO.StreamReader($fs, [System.Text.Encoding]::UTF8); $t = $sr.ReadToEnd(); $sr.Close() }
        finally { $fs.Close() }
        return $t
      }
    } catch { }
    Start-Sleep -Milliseconds 250
  }
  return ""
}

# Kill anything already running from this path so the baseline is a cold start.
Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $runExe } | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

Write-Output ("exe  : " + $Exe)
Write-Output ("copy : " + $runExe)
Write-Output ""

# -- step 1: instance A, and its baseline ------------------------------------
$env:CC_SWITCH_TEST_HOME = $home_
$env:HOME = $home_
try { $a = Start-Process -FilePath $runExe -PassThru -ErrorAction Stop }
catch { Write-Output ("[FAIL] could not launch instance A: " + $_.Exception.Message); exit 2 }
finally { Remove-Item Env:\CC_SWITCH_TEST_HOME, Env:\HOME -ErrorAction SilentlyContinue }

Start-Sleep -Seconds 8
$baseState = [Vis2]::State([uint32]$a.Id)
Info ("step 1: instance A pid " + $a.Id + ", baseline window = " + $baseState)
if ($baseState -eq "VISIBLE") {
  # Cannot attribute a later raise to the second launch if it was already up.
  Fail "instance A's window was already visible before the second launch; this run cannot attribute a later raise"
  Stop-Process -Id $a.Id -Force -ErrorAction SilentlyContinue
  Write-Output ("  failures: " + @($results | Where-Object { $_ -like "FAIL*" }).Count)
  exit 1
}
if ($baseState -ne "hidden") { Fail ("baseline state was '" + $baseState + "', expected 'hidden'") }
else { Pass "baseline: instance A's window is hidden, as silent startup requires" }

# -- step 2: the link-less second launch (NO arguments) ----------------------
$before = Read-LogBestEffort $logPath
$b = Start-Process -FilePath $runExe -PassThru -ErrorAction Stop
Info ("step 2: launched instance B pid " + $b.Id + " with no arguments")
Start-Sleep -Seconds 2
$bAlive = [bool](Get-Process -Id $b.Id -ErrorAction SilentlyContinue)
Info ("        instance B " + $(if ($bAlive) { "is still running as its own process" } else { "exited (its args were forwarded to A)" }))

# -- step 3: sample A across the moment the callback runs --------------------
$sawVisible = $false
$sawForeground = $false
$firstVisibleAt = $null
$t0 = Get-Date
$samples = 0
$deadline = $t0.AddSeconds($Seconds)
while ((Get-Date) -lt $deadline) {
  if (-not (Get-Process -Id $a.Id -ErrorAction SilentlyContinue)) { break }
  $off = [Math]::Round(((Get-Date) - $t0).TotalSeconds, 2)
  $st = [Vis2]::State([uint32]$a.Id)
  if ($st -eq "VISIBLE") {
    $sawVisible = $true
    if ($null -eq $firstVisibleAt) { $firstVisibleAt = $off }
  }
  if ([Vis2]::IsForeground([uint32]$a.Id)) {
    $sawForeground = $true
    Write-Output ("  [t+" + $off + "s] instance A's window is the FOREGROUND window -- focus was taken")
  }
  $samples++
  Start-Sleep -Milliseconds $IntervalMs
}

# Did the callback actually run? If not, this run measured nothing.
$after = Read-LogBestEffort $logPath
$newPart = if ($after.Length -gt $before.Length) { $after.Substring($before.Length) } else { $after }
$callbackRan = $newPart -match 'Single Instance Callback Triggered'
$noDeeplink = $newPart -match 'No deep link URL found in args'
$newWording = $newPart -match 'Single-instance wake carried no deep link'
Info ("callback ran during the second launch = " + $callbackRan + "; link-less = " + $noDeeplink)
Write-Output "  -- lines the second launch added to the app's own log --"
($newPart -split "`r?`n" | Where-Object { $_ -match 'Single|deep link|window' } | Select-Object -First 6) | ForEach-Object { Info ("      " + $_.Trim()) }

Write-Output ""
Write-Output "=== verdict ==="
if (-not $callbackRan -and $bAlive) {
  Fail "INCONCLUSIVE: instance B ran as its own process, so A never received the single-instance callback -- this run proves nothing either way"
} else {
  if (-not $callbackRan) { Info "(the callback line was not found; proceeding since B exited, which implies forwarding)" }
  if ($sawVisible) {
    Fail ("instance A's window became VISIBLE after a link-less second launch (first at +" + $firstVisibleAt + "s) -- this is the owner's flash")
  } else {
    Pass ("instance A's window stayed hidden across " + $samples + " samples after a link-less second launch")
  }
  if ($sawForeground) { Fail "the second launch stole focus from the foreground application" }
  else { Pass "the second launch never took foreground focus" }
}

Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $runExe } | Stop-Process -Force -ErrorAction SilentlyContinue
Stop-Process -Id $b.Id -Force -ErrorAction SilentlyContinue
Write-Output ""
Write-Output "=== summary ==="
$results | ForEach-Object { Write-Output ("  " + $_) }
$failed = @($results | Where-Object { $_ -like "FAIL*" }).Count
Write-Output ("  failures: " + $failed)
Write-Output ("  tree: " + $Work)
exit $(if ($failed -eq 0) { 0 } else { 1 })
