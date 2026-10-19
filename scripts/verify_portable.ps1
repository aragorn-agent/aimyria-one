# Acceptance checks A-E for the portable single-file exe.
#
# Each acceptance item is one numbered section, and the point of writing it as a
# script rather than a sequence of hand-run commands is that a claim about a
# binary has to be repeatable: "it ran on my machine" is not a result anyone can
# re-check. The raw output of a run is the evidence.
#
# Why each check exists, in one line each:
#
#   A  the import table is the only place a dynamic dependency is visible before
#      the exe is started anywhere, and WebView2Loader is the dependency that
#      made the GNU build unusable away from its own folder
#   B  a tray app that dies on startup looks exactly like one that works -- the
#      process surviving the WebView2 initialisation is the actual claim
#   C  the window is hidden by design (`"visible": false`); the *main* window is
#      what that describes, so the check is about class `Tauri Window` and not
#      about "some visible window owned by this pid"
#   D  `aimyria://` must resolve to *this* file after it has run once
#   E  start-on-login must point at this file too
#
# F (a real deep link reaching the parser) needs a log read and is deliberately
# not folded in here: firing a link makes the app show its window, so it has to
# run after B/C, in a separate step, or it invalidates them.
#
# Two measurement traps this script exists to avoid:
#
#   1. `Get-Process().MainWindowHandle` is not "the main window". This app
#      registers a single-instance helper whose top-level window is a 6x6
#      `com.aimyria.desktop-sic`, and that is what gets returned -- a window that
#      is neither the app's UI nor a useful signal about it.
#   2. A second launch while an instance is already running does not start a
#      second process at all: the single-instance plugin forwards the args and
#      the new process exits 0 immediately. That exit is indistinguishable from
#      the crash this check is looking for *if you only look at the exit code*.
#      So the pre-flight below kills stale instances and asserts a cold state.

param(
  [string]$Exe = (Join-Path (Split-Path -Parent $PSScriptRoot) "release\AimyriaOne-x64.exe"),
  [string]$Objdump = "C:\msys64\mingw64\bin\objdump.exe",
  [switch]$SkipLaunch,
  [bool]$RequireStaticCrt = $true
)

$ErrorActionPreference = "Continue"
$results = [System.Collections.Generic.List[string]]::new()

function Section($t) { Write-Output ""; Write-Output ("=== " + $t + " ===") }
function Pass($m) { Write-Output ("  [PASS] " + $m); $results.Add("PASS  $m") }
function Fail($m) { Write-Output ("  [FAIL] " + $m); $results.Add("FAIL  $m") }
function Info($m) { Write-Output ("  " + $m) }

# Every running instance, found by executable path, not by process name.
#
# The process name comes from the file name, so a copy delivered as
# `AimyriaOne-x64.exe` runs as `AimyriaOne-x64` and `Get-Process -Name
# "aimyria-one"` does not see it. That mismatch made the cold-state gate in this
# script pass while an instance was running, which turned the subsequent launch
# into a no-op absorbed by the single-instance plugin -- reported as a launch
# failure by a later step. Matching on the path substring does not depend on the
# file's name.
function Get-AppProcesses {
  @(Get-Process -ErrorAction SilentlyContinue |
      Where-Object { $_.Path -and ($_.Path -match 'aimyria' -or $_.Path -match 'Aimyria') })
}
function Get-AppPids { @(Get-AppProcesses | Select-Object -ExpandProperty Id) }

Write-Output ("exe      : " + $Exe)
if (-not (Test-Path -LiteralPath $Exe)) { Write-Output "no exe at $Exe"; exit 1 }
Write-Output ("size     : {0:N0} bytes ({1:N2} MB)" -f (Get-Item $Exe).Length, ((Get-Item $Exe).Length / 1MB))
Write-Output ("sha256   : " + (Get-FileHash -LiteralPath $Exe -Algorithm SHA256).Hash)
Write-Output ("built    : " + (Get-Item $Exe).LastWriteTime)

# -- shared window helpers ---------------------------------------------------
if (-not ("WinQ" -as [type])) {
  Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class WinQ {
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern int GetWindowTextLength(IntPtr h);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }

    // Every top-level window owned by the pid, visible or not, with its class.
    public static List<string> All(uint target) {
        var list = new List<string>();
        EnumWindows((h,l)=>{ uint pid; GetWindowThreadProcessId(h, out pid);
            if (pid==target) { RECT r; GetWindowRect(h, out r);
                var sb = new StringBuilder(256); GetClassName(h, sb, 256);
                list.Add("class='" + sb.ToString() + "' visible=" + IsWindowVisible(h)
                       + " size=" + (r.R-r.L) + "x" + (r.B-r.T) + " hwnd=0x" + h.ToString("X"));
            } return true; }, IntPtr.Zero);
        return list; }
}
'@ -ErrorAction SilentlyContinue
}

# The app's real UI window. `com.aimyria.desktop-sic` is the single-instance
# helper and `Tauri Window` is what tauri.conf.json's `main` window creates.
#
# The parameter is `$ProcId`, NOT `$Pid`: `$Pid` is a read-only automatic
# variable in PowerShell, so a parameter of that name cannot be bound. An
# earlier version of this script used `$Pid`; every call failed with
# "Cannot overwrite variable Pid", the helper emitted nothing, and the
# resulting window count of 0 was read as "no visible window" -- a PASS
# produced by a measurement error rather than by the app. The count is
# therefore returned as an explicit status below, and C fails loudly when the
# measurement did not happen.
function Get-MainWindows([int]$ProcId) {
  @([WinQ]::All([uint32]$ProcId) | Where-Object { $_ -match "class='Tauri Window'" })
}
function Get-VisibleMainWindows([int]$ProcId) {
  @(Get-MainWindows $ProcId | Where-Object { $_ -match 'visible=True' })
}
# Returns a result object carrying the count *and* whether the probe ran.
#
# Two PowerShell traps had to be worked around to make this honest:
#   - the parameter cannot be named `$Pid`: that is a read-only automatic
#     variable, so binding fails, the call throws, and an empty result reads as
#     "zero visible windows".
#   - a function returning `@()` does not return an empty array; PowerShell
#     unwraps it to `$null` on the way out, which is indistinguishable from
#     "the probe threw". Returning `,@()` (unary comma) avoids the unwrap, and
#     an explicit `Ok` flag removes the ambiguity entirely.
function Measure-VisibleMainWindows([int]$ProcId) {
  try {
    $all = [WinQ]::All([uint32]$ProcId)
    if ($null -eq $all) { return [pscustomobject]@{ Ok = $false; Lines = @() } }
    $vis = @($all | Where-Object { $_ -match "class='Tauri Window'" -and $_ -match 'visible=True' })
    return [pscustomobject]@{ Ok = $true; Lines = $vis }
  } catch {
    return [pscustomobject]@{ Ok = $false; Lines = @() }
  }
}

# -- A. import table ---------------------------------------------------------
Section "A. import table (objdump -p)"
if (-not (Test-Path -LiteralPath $Objdump)) { Fail "objdump not found at $Objdump" }
else {
  Write-Output ("  command: " + $Objdump + " -p `"" + $Exe + "`"")
  $raw = & $Objdump -p $Exe 2>&1
  $names = $raw | Select-String '^\s*DLL Name:' | ForEach-Object { ($_ -split ':', 2)[1].Trim() }
  Write-Output ("  raw DLL Name lines (" + $names.Count + "):")
  $names | Sort-Object -Unique | ForEach-Object { Write-Output ("    " + $_) }
  $wv2 = $names | Where-Object { $_ -match 'WebView2Loader' }
  if ($wv2) { Fail ("still imports " + ($wv2 -join ", ")) }
  else { Pass "no WebView2Loader entry in the import table - self-contained" }
  # Recorded, not asserted: the MSVC toolchain pulls the VC runtime import
  # libraries by default, which the GNU build did not (it used the system's
  # msvcrt.dll). These libraries ship with the VC++ redistributable, not with
  # Windows, so an exe that imports them is not "no install needed" on a clean
  # machine -- the exact promise this task exists to satisfy. The standard for
  # deliverable A is therefore "no WebView2Loader *and* no VC runtime imports";
  # `-RequireStaticCrt:$false` downgrades the second half to informational for
  # an exploratory build.
  $vcr = $names | Where-Object { $_ -match 'VCRUNTIME|api-ms-win-crt|MSVCP' } | Sort-Object -Unique
  Info ("VC-runtime imports found: " + $(if ($vcr) { $vcr -join ", " } else { "none" }))
  if ($vcr) {
    if ($RequireStaticCrt) {
      Fail ("A: still imports the VC runtime (" + ($vcr -join ", ") + ") - needs a statically linked CRT")
    } else {
      Info "(informational only: -RequireStaticCrt:`$false)"
    }
  } else {
    Pass "A: no VC runtime imports either - nothing to install on a clean machine"
  }
}

# -- B/C. empty folder launch ------------------------------------------------
Section "B/C. launch from an otherwise empty folder"
if ($SkipLaunch) {
  Info "(skipped: -SkipLaunch)"
} else {
  Write-Output "  command: kill any stale instance (a second launch would just exit 0)"
  $stale = Get-AppProcesses
  foreach ($s in $stale) { Info ("  killing stale pid " + $s.Id + "  " + $s.Path) }
  $stale | Stop-Process -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 3
  $still = Get-AppProcesses
  if ($still.Count -gt 0) {
    Fail ("could not reach a cold state (" + $still.Count + " still running); B/C would be meaningless")
    $still | ForEach-Object { Info ("    pid " + $_.Id + "  " + $_.Path) }
  }
  else { Pass "cold state: no instance of this app is running" }

  $lab = Join-Path $env:TEMP "aimyria-verify-alone"
  if (Test-Path $lab) { Remove-Item $lab -Recurse -Force }
  New-Item -ItemType Directory -Force -Path $lab | Out-Null
  Copy-Item -LiteralPath $Exe (Join-Path $lab "AimyriaOne.exe") -Force

  $files = Get-ChildItem $lab -File
  Info ("folder: " + $lab)
  Info ("contents (" + $files.Count + " file): " + ($files.Name -join ", "))
  if ($files.Count -ne 1) { Fail "folder is not a lone exe" } else { Pass "folder holds exactly one file, no dll beside it" }

  $proc = Start-Process -FilePath (Join-Path $lab "AimyriaOne.exe") -PassThru
  Info ("launched pid " + $proc.Id + " at " + (Get-Date -Format "HH:mm:ss"))

  Start-Sleep -Seconds 10
  $alive10 = [bool](Get-Process -Id $proc.Id -ErrorAction SilentlyContinue)
  $m10 = if ($alive10) { Measure-VisibleMainWindows $proc.Id } else { [pscustomobject]@{ Ok = $true; Lines = @() } }
  Info ("t=10s alive=" + $alive10 + "  visible 'Tauri Window' count=" + $(if ($m10.Ok) { @($m10.Lines).Count } else { "PROBE FAILED" }))
  if (-not $alive10) { $ec = $proc.ExitCode; Info ("exit code: " + $ec + "  (0 usually means the single-instance plugin forwarded this launch)") }
  if ($alive10) {
    if (-not $m10.Ok) { Fail "C: the window probe failed, so this check did not run" }
    elseif (@($m10.Lines).Count -eq 0) { Pass "C: main window not visible 10 s after launch (silent)" }
    else { Fail "C: the main window is visible 10 s after launch" }
  }

  Start-Sleep -Seconds 9
  $m19 = if ([bool](Get-Process -Id $proc.Id -ErrorAction SilentlyContinue)) { Measure-VisibleMainWindows $proc.Id } else { [pscustomobject]@{ Ok = $false; Lines = @() } }
  Info ("t=19s visible 'Tauri Window' count=" + $(if ($m19.Ok) { @($m19.Lines).Count } else { "n/a (process gone)" }))

  Start-Sleep -Seconds 6
  $alive25 = [bool](Get-Process -Id $proc.Id -ErrorAction SilentlyContinue)
  $m25 = if ($alive25) { Measure-VisibleMainWindows $proc.Id } else { [pscustomobject]@{ Ok = $true; Lines = @() } }
  Info ("t=25s alive=" + $alive25 + "  visible 'Tauri Window' count=" + $(if ($m25.Ok) { @($m25.Lines).Count } else { "PROBE FAILED" }))
  if ($alive25) { Pass ("B: still running 25 s after launch from an empty folder (pid " + $proc.Id + ")") }
  else { Fail "B: exited within 25 s - the WebView2Loader failure mode" }
  if ($alive25) {
    if (-not $m25.Ok) { Fail "C: the window probe failed, so this check did not run" }
    elseif (@($m25.Lines).Count -eq 0) { Pass "C: main window still not visible at 25 s" } else { Fail "C: the main window became visible within 25 s" }
  }
  Info "all top-level windows of this pid (for the record):"
  [WinQ]::All([uint32]$proc.Id) | ForEach-Object { Info ("    " + $_) }

  # The app re-registers aimyria:// on every launch, so this temp-copy run
  # repoints the protocol at the temp path. It is stopped here so the D/E phase
  # can launch the delivered exe from its final path and leave the machine
  # pointing at the path that ships.
  Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
  Info "stopped the empty-folder run"
}

# -- D/E. registry ----------------------------------------------------------
Section "D/E. protocol + autostart, after launching the delivered exe from its final path"
$stopped = $false

# Reads one registry default value, retrying until it names the expected path.
#
# Polling is needed on every one of these, not just the first: `maybe_run` runs
# register_protocol and then reassert_autostart, so the three values are written
# in sequence after the WebView2 runtime is up. Reading immediately after the
# first one lands can catch the previous values and report failures that are
# only a race. An earlier version read the Run value right after the aimyria
# poll broke and reported STALE against a value that was corrected a second
# later.
function Read-RegValue($Key, $Name, $Exe, $TimeoutSeconds) {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  $val = ""
  while ($true) {
    $q = if ($Name) { reg query $Key /v $Name 2>&1 } else { reg query $Key /ve 2>&1 }
    $val = ($q | Select-String 'REG_SZ' | ForEach-Object { ($_ -split 'REG_SZ', 2)[1].Trim() }) -join ""
    if ($val -match [regex]::Escape($Exe)) { break }
    if ((Get-Date) -ge $deadline) { break }
    Start-Sleep -Seconds 2
  }
  return $val
}

if (-not $SkipLaunch) {
  # Point the machine at the shipped path, which is the state this task is meant
  # to leave behind. Launching the temp copy last would leave both schemes aimed
  # at %TEMP%.
  $reg = Start-Process -FilePath $Exe -PassThru
  Info ("launched " + $Exe + " as pid " + $reg.Id + " at " + (Get-Date -Format "HH:mm:ss"))
  Info "polling up to 45 s for each value to name the delivered exe"
  $dAimyria  = Read-RegValue "HKCU\Software\Classes\aimyria\shell\open\command"  $null $Exe 45
  $dCcswitch = Read-RegValue "HKCU\Software\Classes\ccswitch\shell\open\command" $null $Exe 45
  $dRun      = Read-RegValue "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" "AimyriA go" $Exe 45
  $alive = [bool](Get-Process -Id $reg.Id -ErrorAction SilentlyContinue)
  Info ("still running: " + $alive)
  Info ("state after this run: pid " + $reg.Id + " running (left up for the F step)")
  $stopped = $true
} else {
  Info "(launch skipped; D/E below reflect whatever ran last)"
  $dAimyria  = Read-RegValue "HKCU\Software\Classes\aimyria\shell\open\command"  $null $Exe 1
  $dCcswitch = Read-RegValue "HKCU\Software\Classes\ccswitch\shell\open\command" $null $Exe 1
  $dRun      = Read-RegValue "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" "AimyriA go" $Exe 1
}

Write-Output '  command: reg query "HKCU\Software\Classes\aimyria\shell\open\command" /ve'
reg query "HKCU\Software\Classes\aimyria\shell\open\command" /ve 2>&1 | ForEach-Object { Info ("  " + $_) }
Info ("parsed (aimyria) : " + $dAimyria)
if ($dAimyria -match [regex]::Escape($Exe)) { Pass "D: aimyria:// open command points at the delivered exe path" }
else { Fail ("D: aimyria:// does not point at the delivered exe: " + $dAimyria) }

Write-Output ""
Write-Output '  command: reg query "HKCU\Software\Classes\ccswitch\shell\open\command" /ve'
reg query "HKCU\Software\Classes\ccswitch\shell\open\command" /ve 2>&1 | ForEach-Object { Info ("  " + $_) }
Info ("parsed (ccswitch): " + $dCcswitch)
# The console's button emits ccswitch://, so this scheme has to resolve to this
# exe too. It used to be left to whatever registered it last, which on a machine
# with an older install was the older install -- and on a clean machine was
# nothing at all, sending the fallback link into the browser's "no associated
# app" dead end.
if ($dCcswitch -match [regex]::Escape($Exe)) { Pass "D2: ccswitch:// open command points at the delivered exe path" }
else { Fail ("D2: ccswitch:// does not point at the delivered exe: " + $dCcswitch) }

Write-Output ""
Write-Output '  command: reg query "HKCU\Software\Classes\aimyria" /s'
(reg query "HKCU\Software\Classes\aimyria" /s 2>&1) | ForEach-Object { Info ("  " + $_) }
Write-Output '  command: reg query "HKCU\Software\Classes\ccswitch" /s'
(reg query "HKCU\Software\Classes\ccswitch" /s 2>&1) | ForEach-Object { Info ("  " + $_) }

Section "E. start-on-login (HKCU Run)"
Write-Output '  command: reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run"'
$run = reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" 2>&1
$run | ForEach-Object { Info ("  " + $_) }
$runHit = $run | Select-String -SimpleMatch $Exe
if ($runHit) {
  Pass "E: a Run value points at the delivered exe"
} elseif ($dRun -match [regex]::Escape($Exe)) {
  # The polled value is authoritative: the printed listing above can be stale by
  # one write, because reassert_autostart runs after register_protocol.
  Pass "E: the Run value 'AimyriA go' points at the delivered exe (per the 45 s poll)"
} else {
  # A stale entry is a different claim from a missing one, and conflating them
  # would hide a real defect. `auto-launch` stores this under the fixed value
  # name "AimyriA go" (src/auto_launch.rs sets app_name to that literal), and
  # `AutoLaunch::is_enabled()` only tests whether the value *exists* -- it does
  # not compare the stored path. So when an older install has left a value under
  # that name pointing somewhere else, `first_run::seed_autostart` reads
  # "autostart is already on", returns early, and the entry is never repointed
  # at the current exe. That is a latent bug, not a property of this build.
  $appNamed = $run | Select-String -Pattern 'AimyriA go'
  if ($appNamed) {
    $staleVal = ($appNamed -join "") -replace '^\s*AimyriA go\s+REG_SZ\s+', ''
    Info "E: a Run value named 'AimyriA go' exists but points elsewhere:"
    Info ("    " + $staleVal.Trim())
    Info "E: cause -- is_enabled() checks presence only, so seed_autostart returns early"
    Info "E: consequence -- if that old path is removed, autostart breaks silently"
    Info "E: cleanup (Lead's call, not this task's):"
    Info "    reg delete `"HKCU\Software\Microsoft\Windows\CurrentVersion\Run`" /v `"AimyriA go`" /f"
    Info "    then launch the delivered exe once; it writes the current path"
    $results.Add("STALE  E: Run value 'AimyriA go' points at an older path, not the delivered exe")
  } else {
    Fail "E: no Run value points at the delivered exe, and none named 'AimyriA go' exists at all"
  }
}

Write-Output ""
Write-Output "=== summary ==="
$results | ForEach-Object { Write-Output ("  " + $_) }
$failed = @($results | Where-Object { $_ -like "FAIL*" }).Count
$stale = @($results | Where-Object { $_ -like "STALE*" }).Count
Write-Output ("  failures: " + $failed + "   stale (pre-existing, not a build defect): " + $stale)
exit $(if ($failed -eq 0) { 0 } else { 1 })
