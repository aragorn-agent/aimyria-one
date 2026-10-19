# Does the main window own a taskbar button, sampled across startup?
#
# WHY THIS PROBE IS BUILT THE WAY IT IS
#
# Two earlier attempts at this measurement were wrong, and both were wrong in the
# dangerous direction: they reported PASS on a build where the bug was present.
#
# 1. **Extended style.** Reading WS_EX_APPWINDOW looks like the obvious answer, but
#    tao 0.34.6 implements `skip_taskbar` as a COM call, not a style bit:
#
#      let taskbar_list: ITaskbarList = CoCreateInstance(&TaskbarList, ...)?;
#      if skip { taskbar_list.DeleteTab(hwnd)?; } else { taskbar_list.AddTab(hwnd)?; }
#
#    `ITaskbarList::DeleteTab` removes the button through the shell and never
#    touches the window's extended style. So WS_EX_APPWINDOW stays set on a window
#    that has no button at all -- an assertion on it can never pass.
#
# 2. **Taskbar child window titles.** Walking Shell_TrayWnd for a child whose text
#    is the app's title. That finds nothing under any circumstances: on this machine
#    the taskbar's `MSTaskListWClass` containers have no per-button children, so a
#    title match can never succeed. It reported "no button" for the *unfixed* build
#    too -- which is how it was caught. A control run that does not fail is a broken
#    check, not a working fix.
#
# The measurement used now is the shell's own enumeration of taskbar windows
# (`Shell.Application.Windows()`), which lists exactly the windows the taskbar is
# showing. A window created with `skipTaskbar: true` (tao calls DeleteTab during
# construction) is not in that list; one without it is.
#
# A/B DISCIPLINE
#
# Run this against BOTH a build without the fix (expect Present) and the build with
# it (expect Absent). Either run alone proves nothing, and the control is what
# caught the two broken versions above.
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [int]$Seconds = 12,
  [int]$IntervalMs = 120
)

$ErrorActionPreference = "Continue"

Write-Output ("exe      : " + $Exe)
Write-Output ("sampling : every ${IntervalMs}ms for ${Seconds}s")
Write-Output "  measurement: Shell.Application.Windows() taskbar membership (NOT extended style)"
Write-Output ""

# One shell object for the whole run: creating it per sample would be slower than
# the sampling interval and would dominate the measurement.
$shell = New-Object -ComObject Shell.Application

function Get-TaskbarWindows {
  $out = @()
  try {
    foreach ($w in $shell.Windows()) {
      try {
        $h = [int64]$w.HWND
        if ($h -ne 0) { $out += [pscustomobject]@{ HWND = $h; Name = $w.Name } }
      } catch { }
    }
  } catch { }
  return $out
}

$stale = @(Get-Process -ErrorAction SilentlyContinue |
           Where-Object { $_.Path -and ($_.Path -match 'aimyria') })
foreach ($s in $stale) { Write-Output ("  killing stale pid " + $s.Id + "  " + $s.Path) }
$stale | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 3

try {
  $proc = Start-Process -FilePath $Exe -PassThru -ErrorAction Stop
} catch {
  Write-Output ("[FAIL] could not launch: " + $_.Exception.Message)
  exit 2
}
$pid_ = $proc.Id
if ($null -eq $pid_ -or $pid_ -eq 0) { Write-Output "[FAIL] launch returned no pid"; exit 2 }

# Which hwnd is the app's main window? Resolved by class, so the check is about
# *this* window rather than "some window of this process".
Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class MainWnd {
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern int GetWindowLong(IntPtr h, int i);
  public static long Find(uint target) {
    long found = 0;
    EnumWindows((h, l) => {
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (pid != target) return true;
      var sb = new StringBuilder(256); GetClassName(h, sb, 256);
      if (sb.ToString() == "Tauri Window") { found = h.ToInt64(); return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }
  public static int ExStyle(long h) { return GetWindowLong(new IntPtr(h), -20); }
}
'@ -ErrorAction SilentlyContinue

$t0 = Get-Date
Write-Output ("launched pid " + $pid_ + " at " + $t0.ToString("HH:mm:ss.fff"))
Write-Output ""

$samples = 0
$seenCount = 0
$firstSeen = $null
$mainHwnd = 0
$exStyleChanges = 0
$prevEx = $null

$deadline = $t0.AddSeconds($Seconds)
while ((Get-Date) -lt $deadline) {
  if (-not (Get-Process -Id $pid_ -ErrorAction SilentlyContinue)) { break }
  $off = [Math]::Round(((Get-Date) - $t0).TotalSeconds, 2)

  if ($mainHwnd -eq 0) { $mainHwnd = [MainWnd]::Find([uint32]$pid_) }

  # The assertion: is this process's main window in the shell's taskbar list?
  $tb = Get-TaskbarWindows
  $hit = @($tb | Where-Object { $_.HWND -eq $mainHwnd })
  if ($hit.Count -gt 0) {
    $seenCount++
    if ($null -eq $firstSeen) {
      $firstSeen = $off
      Write-Output ("  [t+${off}s] taskbar lists hwnd=0x" + $mainHwnd.ToString("X") + " name='" + $hit[0].Name + "'")
    }
  }

  # Informational only: the extended style, recorded to show that it does NOT move
  # (which is why it cannot be the assertion).
  if ($mainHwnd -ne 0) {
    $ex = [MainWnd]::ExStyle($mainHwnd)
    if ($null -ne $prevEx -and $ex -ne $prevEx) {
      $exStyleChanges++
      Write-Output ("  [exStyle +${off}s] 0x" + ('{0:X}' -f $prevEx) + " -> 0x" + ('{0:X}' -f $ex))
    }
    $prevEx = $ex
  }

  $samples++
  Start-Sleep -Milliseconds $IntervalMs
}

$elapsed = [Math]::Round(((Get-Date) - $t0).TotalSeconds, 2)
Write-Output ""
Write-Output ("=== sampled $samples times over ${elapsed}s; main hwnd=0x" + $mainHwnd.ToString("X") + "; exStyle changes=$exStyleChanges ===")
$verdict = if ($seenCount -gt 0) { "True" } else { "False" }
Write-Output ("RESULT taskbarButtonForMainWindow=" + $verdict +
              "  samplesWithButton=$seenCount/$samples" +
              "  firstSeenAt=+" + $(if ($null -ne $firstSeen) { $firstSeen } else { "never" }) + "s")
if ($seenCount -eq 0) {
  Write-Output "  [PASS] the shell never listed the main window in the taskbar during startup"
} else {
  Write-Output ("  [FAIL] the main window was in the taskbar list (first at +${firstSeen}s) -- the owner's flash")
}

Stop-Process -Id $pid_ -Force -ErrorAction SilentlyContinue
Write-Output ("  stopped pid " + $pid_)
