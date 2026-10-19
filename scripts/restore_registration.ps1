# Snapshot / restore / verify this app's three system-registration values.
#
# WHY THIS SCRIPT EXISTS
#
# `first_run::maybe_run` re-registers `aimyria://`, `ccswitch://` and the
# `HKCU\...\Run` value on **every launch**, pointing them at
# `std::env::current_exe()`. That is correct behaviour for an installed app and a
# landmine for anyone testing one: start a copy from %TEMP% and it silently promotes
# itself to the machine's protocol handler. Delete the copy, and every future
# 「导入到CCS」 click on this machine resolves to a file that no longer exists.
#
# This has now happened twice in this project (a copy left in %TEMP% by a probe,
# registry repointed, directory cleaned up, registration left dangling). The fix is
# not "remember to restore it" -- it is to make save/restore a single command that
# every test calls before and after:
#
#   pwsh -File scripts\restore_registration.ps1 -Save          # before: snapshot
#   pwsh -File scripts\restore_registration.ps1 -Restore       # after:  put back
#   pwsh -File scripts\restore_registration.ps1 -Verify        # any time: is it sane?
#
# `-Verify` is the part that matters most: it answers "do all three point at a file
# that actually exists", which is the failure nobody notices until an import stops
# working.
#
# WHY THE .NET REGISTRY API RATHER THAN `reg add`
#
# `reg add ... /d '"C:\path\app.exe" "%1"'` was measured dropping the leading quote,
# storing `C:\path\app.exe "%1"` -- which still works for paths without spaces and
# breaks for paths with them (`C:\Program Files\...`). Writing through
# `Microsoft.Win32.Registry` with an explicit string value stores exactly the
# characters given. `reg add` also hung for 120 s on one invocation in this project.
param(
  [string]$SnapshotPath = "",
  [switch]$Save,
  [switch]$Restore,
  [switch]$Verify,
  # List every launchable copy of this app under the usual test locations.
  #
  # A copy of this exe is not inert: `first_run::maybe_run` re-registers both
  # protocol schemes and the Run value on launch, pointing them at
  # `current_exe()`. So a leftover copy in %TEMP% is a live trigger for the exact
  # incident this script exists to clean up -- someone (a probe, a script, an
  # autostart entry) runs it, and the machine's 「导入到CCS」 silently points into
  # TEMP. Twice in this project a copy was left behind by a test whose cleanup
  # failed quietly; the registry was dangling both times.
  #
  # `-Scan` is meant for the `finally` block of any test that launches a copy.
  [switch]$Scan,
  [string]$ExpectedExe = ""
)

$ErrorActionPreference = "Continue"
$failed = 0
function Pass($m) { Write-Output ("  [PASS] " + $m) }
function Fail($m) { Write-Output ("  [FAIL] " + $m); $script:failed++ }
function Info($m) { Write-Output ("  " + $m) }

function Invoke-Scan {
  # Bounded on purpose: a recursive walk of %TEMP% or Downloads on this machine took
  # minutes (it contains whole build trees). Depth 3 catches every layout a test
  # actually produces -- `%TEMP%\<name>\AimyriaOne.exe` and
  # `%TEMP%\<name>\release\...` -- without walking into the middle of a toolchain.
  $roots = @($env:TEMP, $env:TMP) | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -Unique
  $found = @()
  foreach ($root in $roots) {
    $queue = @(@{ Path = $root; Depth = 0 })
    while ($queue.Count -gt 0) {
      $cur = $queue[0]; $queue = @($queue | Select-Object -Skip 1)
      $found += @(Get-ChildItem -LiteralPath $cur.Path -File -ErrorAction SilentlyContinue |
                  Where-Object { $_.Name -match '^(AimyriaOne|aimyria-one).*\.exe$' })
      if ($cur.Depth -ge 3) { continue }
      foreach ($d in @(Get-ChildItem -LiteralPath $cur.Path -Directory -ErrorAction SilentlyContinue)) {
        # Skip the obvious toolchain caches: they are large and never hold a test copy.
        if ($d.Name -match '^(node_modules|\.git|target|\.cargo|\.rustup)$') { continue }
        $queue += @{ Path = $d.FullName; Depth = $cur.Depth + 1 }
      }
    }
  }
  Write-Output ("=== launchable copies of this app under TEMP (depth 3): " + $found.Count + " ===")
  foreach ($f in $found) { Info ($f.FullName + "   " + $f.Length + " bytes") }
  if ($found.Count -eq 0) {
    Pass "no stray launchable copy found"
  } else {
    Fail ($found.Count.ToString() + " stray copy/copies present -- running ANY of them re-points aimyria://, ccswitch:// and the Run value at that path")
  }
  return $found.Count
}

$AIMYRIA = "HKCU:\Software\Classes\aimyria\shell\open\command"
$CCSWITCH = "HKCU:\Software\Classes\ccswitch\shell\open\command"
$RUN_KEY = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$RUN_NAME = "AimyriA go"

if (-not $SnapshotPath) {
  $SnapshotPath = Join-Path (Split-Path -Parent $PSScriptRoot) ".registration-snapshot.json"
}

function Read-Reg($key, $name) {
  try {
    $item = Get-ItemProperty -LiteralPath $key -ErrorAction Stop
    if ($name) { return $item.$name }
    return $item.'(default)'
  } catch { return $null }
}

function Write-RegDefault($key, $value) {
  $sub = $key
  if ($sub.StartsWith("HKCU:\")) { $sub = $sub.Substring(6) }
  $k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($sub, $true)
  if ($null -eq $k) {
    $k = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($sub)
  }
  try { $k.SetValue('', $value, [Microsoft.Win32.RegistryValueKind]::String) }
  finally { $k.Close() }
}

function Write-RegValue($key, $name, $value) {
  $sub = $key
  if ($sub.StartsWith("HKCU:\")) { $sub = $sub.Substring(6) }
  $k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($sub, $true)
  if ($null -eq $k) {
    $k = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($sub)
  }
  try { $k.SetValue($name, $value, [Microsoft.Win32.RegistryValueKind]::String) }
  finally { $k.Close() }
}

# Extract the executable path from a `"C:\path\app.exe" "%1"` style command.
function Get-ExeFromCommand($command) {
  if (-not $command) { return "" }
  $m = [regex]::Match($command, '^\s*"([^"]+)"')
  if ($m.Success) { return $m.Groups[1].Value }
  $m = [regex]::Match($command, '^\s*(\S+)')
  if ($m.Success) { return $m.Groups[1].Value }
  return ""
}

# ---------------------------------------------------------------------------
if ($Save) {
  $snapshot = [ordered]@{
    savedAt = (Get-Date).ToString("o")
    aimyria = Read-Reg $AIMYRIA $null
    ccswitch = Read-Reg $CCSWITCH $null
    run = Read-Reg $RUN_KEY $RUN_NAME
  }
  $snapshot | ConvertTo-Json | Set-Content -LiteralPath $SnapshotPath -Encoding UTF8
  Write-Output ("=== snapshot saved to " + $SnapshotPath + " ===")
  Write-Output ("  aimyria  : " + $snapshot.aimyria)
  Write-Output ("  ccswitch : " + $snapshot.ccswitch)
  Write-Output ("  Run      : " + $snapshot.run)
  exit 0
}

if ($Restore) {
  if (-not (Test-Path -LiteralPath $SnapshotPath)) {
    Write-Output ("no snapshot at " + $SnapshotPath + " -- run with -Save before testing")
    exit 2
  }
  $snapshot = Get-Content -LiteralPath $SnapshotPath -Raw -Encoding UTF8 | ConvertFrom-Json

  # Refuse to restore a snapshot whose target has since been deleted. Restoring a
  # dangling path is exactly the incident this script exists to prevent.
  $exe = Get-ExeFromCommand $snapshot.aimyria
  if (-not $exe -or -not (Test-Path -LiteralPath $exe)) {
    Write-Output ("[FAIL] the snapshot points at something that no longer exists: `"$exe`"")
    Write-Output "       refusing to restore a dangling registration. Pass a live target instead:"
    Write-Output "       pwsh -File scripts\restore_registration.ps1 -Restore -ExpectedExe <path>"
    if (-not $ExpectedExe) { exit 1 }
  }

  if ($ExpectedExe) {
    $q = [char]34
    $cmd = $q + $ExpectedExe + $q + ' ' + $q + '%1' + $q
    Write-RegDefault $AIMYRIA $cmd
    Write-RegDefault $CCSWITCH $cmd
    Write-RegValue $RUN_KEY $RUN_NAME $ExpectedExe
    Write-Output ("=== registration forced onto " + $ExpectedExe + " ===")
  } else {
    if ($snapshot.aimyria) { Write-RegDefault $AIMYRIA $snapshot.aimyria }
    if ($snapshot.ccswitch) { Write-RegDefault $CCSWITCH $snapshot.ccswitch }
    if ($snapshot.run) { Write-RegValue $RUN_KEY $RUN_NAME $snapshot.run }
    Write-Output ("=== registration restored from " + $SnapshotPath + " ===")
  }

  $Verify = $true
}

if ($Verify) {
  Write-Output "=== registration state ==="
  $a = Read-Reg $AIMYRIA $null
  $c = Read-Reg $CCSWITCH $null
  $r = Read-Reg $RUN_KEY $RUN_NAME
  Write-Output ("  aimyria  : " + $a)
  Write-Output ("  ccswitch : " + $c)
  Write-Output ("  Run      : " + $r)

  foreach ($pair in @(@{ n = "aimyria"; v = $a }, @{ n = "ccswitch"; v = $c }, @{ n = "Run"; v = $r })) {
    $exe = Get-ExeFromCommand $pair.v
    if (-not $exe) { Fail ($pair.n + ": value is empty or unparseable"); continue }
    $base = Split-Path -Parent $exe
    if (Test-Path -LiteralPath $exe) {
      Pass ($pair.n + ": target exists -> " + $exe)
    } elseif ($base -and -not (Test-Path -LiteralPath $base)) {
      Fail ($pair.n + ": target's DIRECTORY is gone -> " + $exe + "   (a test copy was deleted; restore to the real install)")
    } else {
      Fail ($pair.n + ": target file missing -> " + $exe)
    }
    # A copy under TEMP is a test artefact, never a legitimate registration.
    if ($exe -match '[\\/]Temp[\\/]') {
      Fail ($pair.n + ": points into a TEMP directory -- this is a test copy, not an install")
    }
  }

  $procs = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and ($_.Path -match 'aimyria') })
  Write-Output ("  running aimyria processes: " + $procs.Count)
  $procs | ForEach-Object { Info ("    " + $_.Id + "  " + $_.Path) }

  $tempCopies = @(Get-ChildItem $env:TEMP -Directory -ErrorAction SilentlyContinue |
                  Where-Object { $_.Name -match '^(aimyria|tb2|flash-probe|lead-verify|reltest)' })
  Write-Output ("  aimyria/test directories left under TEMP: " + $tempCopies.Count)
  $tempCopies | ForEach-Object { Info ("    " + $_.FullName) }

  $Scan = $true
}

if ($Scan) {
  # Run on -Scan, and automatically as the last step of -Verify: "are the three
  # values sane" is only half the question; "is there something lying around that
  # will make them insane again" is the other half.
  [void](Invoke-Scan)
}

if ($Verify -or $Scan -or $Restore) {
  Write-Output ""
  Write-Output ("  failures: " + $failed)
  exit $(if ($failed -eq 0) { 0 } else { 1 })
}

if (-not ($Save -or $Restore -or $Verify)) {
  Write-Output "usage: restore_registration.ps1 -Save | -Restore [-ExpectedExe <path>] | -Verify"
  exit 2
}
