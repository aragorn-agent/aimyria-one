# Requirement 7 acceptance: does anything appear on screen during a silent start?
#
# WHY THIS SAMPLES *EVERY* TOP-LEVEL WINDOW, NOT JUST THE MAIN ONE
#
# The existing C check in verify_portable.ps1 filters on `class='Tauri Window'` and
# asserts the main window is not visible. That is the right check for "did the tray
# app open its UI unasked", and it passes on a correct build -- but it is also
# *blind* to the owner's actual complaint. Enumerating without the filter on this
# build shows two more top-level windows that report `IsWindowVisible == true`:
#
#   class='com.aimyria.desktop-sic'  visible=True  16x16  exStyle=0x80800A0
#   class='Tao Thread Event Target'  visible=True  16x16  exStyle=0x80800A0
#
# The owner's words were "打开的时候、运行的时候会一直闪几个窗口，连续闪几个" --
# *several* windows, flashing *repeatedly*. Those two are "several", and every
# protocol activation / single-instance forward is a fresh opportunity for them to
# appear. So the assertion has to be about **all** windows, which is what this
# script does: it samples every top-level window of the process every ~40 ms and
# reports each window that was EVER visible, with its first and last sighting.
#
# The distinction that matters, and that a coarser check cannot make:
#   - a window that is visible with a 0x0 or 16x16 rect and WS_EX_TRANSPARENT is a
#     plumbing artefact: it is on the "visible" list to Windows but cannot paint and
#     cannot be clicked. Reporting it as a flash would be a false positive;
#   - a window with a real rect (hundreds of px) that becomes visible is a real
#     window on screen, and that is what the owner is complaining about.
# Both are reported, and which is which is decided by the rect and the extended
# style rather than by the class name.
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [int]$Seconds = 25,
  [int]$IntervalMs = 40,
  [switch]$KeepRunning
)

$ErrorActionPreference = "Continue"

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class WinSweep {
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern int GetWindowLong(IntPtr h, int index);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    // WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_APPWINDOW
    const int EX_LAYERED = 0x00080000, EX_TRANSPARENT = 0x00000020;
    const int EX_TOOLWINDOW = 0x00000080, EX_NOACTIVATE = 0x08000000, EX_APPWINDOW = 0x00040000;

    public class W {
        public string Class; public bool Visible; public int W_; public int H_;
        public int Ex; public string Hwnd;
        public bool CanPaint { get { return W_ > 32 && H_ > 32 && (Ex & EX_TRANSPARENT) == 0; } }
        public bool IsTool { get { return (Ex & EX_TOOLWINDOW) != 0; } }
        // The taskbar dimension. Windows shows a taskbar button for a top-level
        // window that has WS_EX_APPWINDOW and not WS_EX_TOOLWINDOW -- regardless of
        // whether the window is visible. That is the whole mechanism behind the
        // owner's "the taskbar icon flashes at startup".
        public bool HasTaskbarButton {
            get { return (Ex & EX_APPWINDOW) != 0 && (Ex & EX_TOOLWINDOW) == 0; }
        }
        public override string ToString() {
            return string.Format("class='{0}' hwnd={1} visible={2} size={3}x{4} exStyle=0x{5:X} toolwindow={6} appwindow={7} taskbarButton={8}",
                Class, Hwnd, Visible, W_, H_, Ex, IsTool,
                (Ex & EX_APPWINDOW) != 0, HasTaskbarButton);
        }
    }

    public static List<W> Snapshot(uint target) {
        var list = new List<W>();
        EnumWindows((h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (pid != target) return true;
            RECT r; GetWindowRect(h, out r);
            var sb = new StringBuilder(256); GetClassName(h, sb, 256);
            list.Add(new W {
                Class = sb.ToString(), Visible = IsWindowVisible(h),
                W_ = r.R - r.L, H_ = r.B - r.T,
                Ex = GetWindowLong(h, -20), Hwnd = "0x" + h.ToString("X")
            });
            return true;
        }, IntPtr.Zero);
        return list;
    }
}
'@ -ErrorAction SilentlyContinue

function Get-AppProcesses {
  @(Get-Process -ErrorAction SilentlyContinue |
      Where-Object { $_.Path -and $_.Path.StartsWith($Exe.Substring(0, $Exe.LastIndexOf('\') + 1), [StringComparison]::OrdinalIgnoreCase) })
}

Write-Output ("exe      : " + $Exe)
Write-Output ("sampling : every ${IntervalMs}ms for ${Seconds}s, ALL top-level windows of the pid")
Write-Output ""

# Cold state, or the launch is absorbed by the single-instance plugin and we end up
# watching the wrong process.
$stale = Get-AppProcesses
if ($stale.Count -gt 0) {
  Write-Output ("  killing " + $stale.Count + " stale instance(s) for a cold state")
  $stale | Stop-Process -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 3
}

$proc = Start-Process -FilePath $Exe -PassThru
$pid_ = $proc.Id
Write-Output ("launched pid " + $pid_ + " at " + (Get-Date -Format "HH:mm:ss.fff"))

# hwnd -> record
$seen = @{}
$samples = 0
$t0 = Get-Date
$deadline = $t0.AddSeconds($Seconds)

# The taskbar trace, sampled per window: "does this window currently own a taskbar
# button, and did that ever change?" `transitions` on the main window is the number
# the fix is judged by -- 0 means the style never moved, which is what "never
# appeared on the taskbar at all" looks like from the outside.
$everTaskbar = @{}
$transitions = 0
$prevTaskbar = @{}

while ((Get-Date) -lt $deadline) {
  if (-not (Get-Process -Id $pid_ -ErrorAction SilentlyContinue)) {
    Write-Output ("  process exited at +" + [Math]::Round(((Get-Date) - $t0).TotalSeconds, 2) + "s")
    break
  }
  foreach ($w in [WinSweep]::Snapshot([uint32]$pid_)) {
    $key = $w.Hwnd
    if (-not $seen.ContainsKey($key)) {
      $seen[$key] = [pscustomobject]@{
        Hwnd = $key; Class = $w.Class; MaxW = $w.W_; MaxH = $w.H_; Ex = $w.Ex
        EverVisible = $false; FirstVisibleAt = $null; LastVisibleAt = $null; Samples = 0
        VisibleSize = "0x0"; CanPaintEver = $false
        FirstEx = $w.Ex; EverAppWindow = $false; EverToolWindow = $false
        EverTaskbarButton = $false; TaskbarTransitions = 0
      }
      $prevTaskbar[$key] = $w.HasTaskbarButton
    }
    $rec = $seen[$key]
    $rec.Samples++
    $offset = [Math]::Round(((Get-Date) - $t0).TotalSeconds, 2)
    if ($w.Visible) {
      $rec.EverVisible = $true
      if ($null -eq $rec.FirstVisibleAt) { $rec.FirstVisibleAt = $offset }
      $rec.LastVisibleAt = $offset
      # The largest rect seen while visible is what decides whether this could have
      # been perceivable on screen.
      if ($w.W_ -gt $rec.MaxW) { $rec.MaxW = $w.W_ }
      if ($w.H_ -gt $rec.MaxH) { $rec.MaxH = $w.H_ }
      $rec.VisibleSize = "$($w.W_)x$($w.H_)"
      if ($w.CanPaint) { $rec.CanPaintEver = $true }
    }
    if (($w.Ex -band 0x00040000) -ne 0) { $rec.EverAppWindow = $true }
    if (($w.Ex -band 0x00000080) -ne 0) { $rec.EverToolWindow = $true }
    if ($w.HasTaskbarButton) { $rec.EverTaskbarButton = $true }
    if ($w.HasTaskbarButton -ne $prevTaskbar[$key]) {
      $rec.TaskbarTransitions++
      $transitions++
      Write-Output ("  [transition +${offset}s] " + $w.Class + " hwnd=" + $key + " taskbarButton " + $prevTaskbar[$key] + " -> " + $w.HasTaskbarButton + " (exStyle=0x" + ('{0:X}' -f $w.Ex) + ")")
      $prevTaskbar[$key] = $w.HasTaskbarButton
    }
  }
  $samples++
  Start-Sleep -Milliseconds $IntervalMs
}

$elapsed = [Math]::Round(((Get-Date) - $t0).TotalSeconds, 2)
Write-Output ""
Write-Output ("=== windows seen (" + $seen.Count + " distinct) after $samples samples in ${elapsed}s ===")
foreach ($rec in ($seen.Values | Sort-Object Class)) {
  $painting = if ($rec.CanPaintEver) { "YES -- real window on screen" } else { "no (transparent/tiny/zero-size)" }
  Write-Output ("  " + $rec.Class)
  Write-Output ("      hwnd=$($rec.Hwnd) everVisible=$($rec.EverVisible) largestVisibleRect=$($rec.VisibleSize) firstEx=0x$('{0:X}' -f $rec.FirstEx)")
  Write-Output ("      everAppWindow=$($rec.EverAppWindow) everToolWindow=$($rec.EverToolWindow) everTaskbarButton=$($rec.EverTaskbarButton) taskbarTransitions=$($rec.TaskbarTransitions)")
  if ($rec.EverVisible) {
    Write-Output ("      firstVisible=+$($rec.FirstVisibleAt)s lastVisible=+$($rec.LastVisibleAt)s")
  }
  Write-Output ("      COULD THE USER HAVE SEEN IT: " + $painting)
}

# The verdict the requirement actually asks for: nothing that can paint was ever
# visible. Tiny/transparent plumbing windows are reported separately, not conflated.
$painters = @($seen.Values | Where-Object { $_.CanPaintEver })
$visibleAny = @($seen.Values | Where-Object { $_.EverVisible })

Write-Output ""
Write-Output "=== verdict ==="
if ($painters.Count -eq 0) {
  Write-Output "  [PASS] no window that could paint was visible at any sample"
} else {
  Write-Output ("  [FAIL] " + $painters.Count + " window(s) could have been seen on screen:")
  $painters | ForEach-Object { Write-Output ("         " + $_.Class + " " + $_.VisibleSize + " first=+" + $_.FirstVisibleAt + "s") }
}
if ($visibleAny.Count -eq 0) {
  Write-Output "  [PASS] no top-level window of this process reported IsWindowVisible=true at any sample"
} else {
  Write-Output ("  [INFO] " + $visibleAny.Count + " window(s) reported visible=True; of those, the ones that can paint are listed above")
}

# -- the taskbar dimension: the owner's actual complaint ----------------------
# The main window is `class='Tauri Window'`. For it to never have had a taskbar
# button, its WS_EX_APPWINDOW must never have been set and its WS_EX_TOOLWINDOW
# must have been set throughout. This is the assertion that the "taskbar icon
# flashes at startup" bug is gone -- and it is checked independently of visibility,
# because a HIDDEN window with an APPWINDOW style still owns a taskbar button.
$main = $seen.Values | Where-Object { $_.Class -eq 'Tauri Window' } | Select-Object -First 1
Write-Output ""
Write-Output "=== taskbar (requirement 7 root cause) ==="
if ($null -eq $main) {
  Write-Output "  [FAIL] the main window (class='Tauri Window') was never enumerated, so this check did not run"
} else {
  Write-Output ("  main window firstExStyle = 0x" + ('{0:X}' -f $main.FirstEx))
  Write-Output ("  everAppWindow  = " + $main.EverAppWindow + "   (must be False: APPWINDOW is what earns a taskbar button)")
  Write-Output ("  everToolWindow = " + $main.EverToolWindow + "   (must be True: ToolWindow windows never get one)")
  Write-Output ("  taskbarTransitions = " + $main.TaskbarTransitions + "   (0 = the style never moved)")
  Write-Output ("  total transitions across all windows = " + $transitions)
  if (-not $main.EverAppWindow -and $main.EverToolWindow -and $main.TaskbarTransitions -eq 0) {
    Write-Output "  [PASS] the main window never had a taskbar button at any sample (never APPWINDOW, always ToolWindow, 0 transitions)"
  } else {
    Write-Output "  [FAIL] the main window had taskbar-button eligibility during startup -- this is the flash the owner reported"
  }
}

if (-not $KeepRunning) {
  Stop-Process -Id $pid_ -Force -ErrorAction SilentlyContinue
  Write-Output "  stopped the sampled instance"
} else {
  Write-Output ("  left running: pid " + $pid_)
}
