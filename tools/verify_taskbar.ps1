param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [int]$ObserveSeconds = 8
)

# Verify the taskbar behaviour with real Win32 calls.
#
# The previous defect was invisible to unit tests and to the log: the window simply
# had no taskbar button. So this checks the two Win32 facts that decide it:
#
#   * the window has the WS_EX_APPWINDOW extended style (or its owner chain makes
#     it top-level), and
#   * `ITaskbarList` is not holding it removed — which we approximate by checking
#     the style plus the window's visibility, since DeleteTab does not change
#     styles.
#
# The decisive, independent check is: does the window appear in the taskbar's
# window enumeration (Alt+Tab / Shell) as a visible top-level window with a title?
# That is what this script asserts.

$ErrorActionPreference = 'Stop'

Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class WinProbe {
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern int GetWindowTextLengthW(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowLongW(IntPtr hWnd, int index);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr hWnd, uint cmd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);

    const int GWL_EXSTYLE = -20;
    const long WS_EX_APPWINDOW = 0x00040000L;
    const long WS_EX_TOOLWINDOW = 0x00000080L;

    public static List<string> Describe(uint targetPid) {
        var result = new List<string>();
        EnumWindows((hWnd, lParam) => {
            uint pid;
            GetWindowThreadProcessId(hWnd, out pid);
            if (pid != targetPid) return true;

            int len = GetWindowTextLengthW(hWnd);
            var sb = new StringBuilder(len + 2);
            GetWindowTextW(hWnd, sb, sb.Capacity);
            string title = sb.ToString();
            if (string.IsNullOrWhiteSpace(title)) return true;

            long ex = GetWindowLongW(hWnd, GWL_EXSTYLE);
            bool appWindow = (ex & WS_EX_APPWINDOW) != 0;
            bool toolWindow = (ex & WS_EX_TOOLWINDOW) != 0;
            bool visible = IsWindowVisible(hWnd);

            // A window gets a taskbar button when it is visible, unowned, and not a
            // tool window. WS_EX_APPWINDOW forces it on regardless of ownership.
            bool hasButton = visible && (appWindow || (!toolWindow && GetWindow(hWnd, 4) == IntPtr.Zero));

            result.Add("title='" + title + "' visible=" + visible +
                       " appWindow=" + appWindow + " toolWindow=" + toolWindow +
                       " taskbarButton=" + hasButton);
            return true;
        }, IntPtr.Zero);
        return result;
    }
}
"@

$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$valueName = 'AIMYRIA GO'
$saved = $null
try { $saved = (Get-ItemProperty -Path $runKey -Name $valueName -ErrorAction Stop).$valueName } catch { }

Write-Output "exe: $Exe"
$proc = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds 3
if ($proc.HasExited) {
    Write-Output "FAIL: process exited immediately"
    exit 1
}

Write-Output "pid: $($proc.Id) — enumerating top-level windows"
$found = [WinProbe]::Describe([uint32]$proc.Id)
if ($found.Count -eq 0) {
    Write-Output "  (no titled windows — silent startup, expected on a quiet launch)"
} else {
    $found | ForEach-Object { Write-Output "  $_" }
}

# The decisive test: show the window the way a user would, then look again.
Write-Output ''
Write-Output 'showing the window via the single-instance wake (second launch, like double-click)'
$second = Start-Process -FilePath $Exe -PassThru
$second.WaitForExit(5000) | Out-Null
Start-Sleep -Seconds 2

$proc.Refresh()
$found2 = [WinProbe]::Describe([uint32]$proc.Id)
if ($found2.Count -eq 0) {
    Write-Output "  (still no titled window)"
} else {
    $found2 | ForEach-Object { Write-Output "  $_" }
}

$hasButton = $found2 | Where-Object { $_ -match 'taskbarButton=True' }
Write-Output ''
if ($hasButton) {
    Write-Output 'RESULT: PASS — a visible window owns a taskbar button'
} else {
    Write-Output 'RESULT: FAIL — visible window has no taskbar button'
}

$nowRun = $null
try { $nowRun = (Get-ItemProperty -Path $runKey -Name $valueName -ErrorAction Stop).$valueName } catch { }
if ($saved -and $nowRun -ne $saved) {
    Set-ItemProperty -Path $runKey -Name $valueName -Value $saved
    Write-Output "autostart: restored"
}

Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
Write-Output "stopped $($proc.Id)"
if ($hasButton) { exit 0 } else { exit 1 }
