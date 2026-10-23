param(
    [Parameter(Mandatory = $true)][string]$Exe
)

# End-to-end: the X button puts the window away and the app keeps running.
#
# The full round trip that matters for "常驻":
#   1. launch, show the window (second launch = double-click);
#   2. send WM_CLOSE — exactly what the X button sends;
#   3. assert the process is still alive and the window is hidden;
#   4. wake it the way the tray icon does (single-instance callback), and
#      assert the window is visible with a taskbar button again.
#
# Step 4 is what separates "verified" from "it did not exit": a process that hides
# the window but cannot bring it back is not usable.

$ErrorActionPreference = 'Stop'

Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class W {
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr p);
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr p);
    [DllImport("user32.dll")] public static extern int GetWindowTextLengthW(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowLongW(IntPtr hWnd, int i);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr hWnd, uint cmd);
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr hWnd, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll", EntryPoint = "IsWindowVisible")] public static extern bool ProbeVisible(IntPtr hWnd);

    const int GWL_EXSTYLE = -20;
    const long WS_EX_APPWINDOW = 0x00040000L;
    const long WS_EX_TOOLWINDOW = 0x00000080L;

    public class Info { public IntPtr Hwnd; public bool Visible; public bool Taskbar; }

    public static List<Info> Windows(uint pid) {
        var list = new List<Info>();
        EnumWindows((h, p) => {
            uint owner; GetWindowThreadProcessId(h, out owner);
            if (owner != pid) return true;
            if (GetWindowTextLengthW(h) == 0) return true;
            long ex = GetWindowLongW(h, GWL_EXSTYLE);
            bool vis = IsWindowVisible(h);
            bool app = (ex & WS_EX_APPWINDOW) != 0;
            bool tool = (ex & WS_EX_TOOLWINDOW) != 0;
            bool hasBtn = vis && (app || (!tool && GetWindow(h, 4) == IntPtr.Zero));
            list.Add(new Info { Hwnd = h, Visible = vis, Taskbar = hasBtn });
            return true;
        }, IntPtr.Zero);
        return list;
    }
}
"@

$settings = Get-Content (Join-Path $env:USERPROFILE '.aimyria\settings.json') -Raw -Encoding UTF8 | ConvertFrom-Json
Write-Output "minimizeToTrayOnClose = $($settings.minimizeToTrayOnClose)"
if (-not $settings.minimizeToTrayOnClose) {
    Write-Output 'RESULT: SKIP — the setting says quit on close, so this test does not apply'
    exit 0
}

$proc = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds 3
if ($proc.HasExited) { Write-Output 'FAIL: died on start'; exit 1 }

$second = Start-Process -FilePath $Exe -PassThru
$second.WaitForExit(5000) | Out-Null
Start-Sleep -Seconds 2
$proc.Refresh()

$shown = [W]::Windows([uint32]$proc.Id) | Where-Object { $_.Visible }
if (-not $shown) { Write-Output 'FAIL: window never became visible'; Stop-Process -Id $proc.Id -Force; exit 1 }
$hwnd = $shown[0].Hwnd
Write-Output "1. shown: visible=True taskbarButton=$($shown[0].Taskbar)"

[W]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Start-Sleep -Seconds 3
$proc.Refresh()

$alive = -not $proc.HasExited
# Ask the *same* hwnd whether it is visible.
#
# The earlier version re-enumerated all windows and counted "titled" ones, which
# is a different question: a hidden window keeps its title, so the count stayed
# non-zero and the test reported a failure for correct behaviour. The property to
# assert is `IsWindowVisible(hwnd) == false`.
$visibleAfter = [W]::ProbeVisible($hwnd)
Write-Output "2. after X: alive=$alive windowVisible=$visibleAfter"

if (-not $alive -or $visibleAfter) {
    Write-Output 'RESULT: FAIL — X should hide the window and keep the process'
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    exit 1
}

# Wake it the way a tray click does: a second launch reaches the single-instance
# callback, which calls tray::show_main_window.
$third = Start-Process -FilePath $Exe -PassThru
$third.WaitForExit(5000) | Out-Null
Start-Sleep -Seconds 2
$proc.Refresh()

$back = [W]::Windows([uint32]$proc.Id) | Where-Object { $_.Visible }
$ok = ($back | Where-Object { $_.Taskbar } | Measure-Object).Count -gt 0
Write-Output "3. wake: visible=$(($back | Measure-Object).Count -gt 0) taskbarButton=$ok"

Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
if ($ok) {
    Write-Output 'RESULT: PASS — X hides to tray, process survives, and it comes back with a taskbar button'
    exit 0
}
Write-Output 'RESULT: FAIL — window came back without a taskbar button'
exit 1
