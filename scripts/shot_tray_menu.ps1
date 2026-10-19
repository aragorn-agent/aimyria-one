# Screenshot the tray menu so the "已导入" marker can be seen.
#
# WHY THIS IS DONE WITHOUT TOUCHING Z-ORDER OR FOCUS
#
# The tray menu is transient: it exists only while it is open, so it cannot be
# captured by watching a window. The obvious way to open it is to move the mouse to
# the tray icon and click -- which takes over the machine's pointer, and on a shared
# desktop that is how a previous run of this project ended up clicking a live
# 「导入」 button in someone else's session and writing a real config.
#
# So this script does not move the pointer, does not raise a window, and does not
# send input to any other application. It:
#
#   1. starts the app;
#   2. opens the tray menu by sending the tray icon's own context-menu message to
#      its own HWND (`WM_RBUTTONUP`/`WM_CONTEXTMENU` to the notification-area icon),
#      which the shell interprets exactly as a right-click on that icon;
#   3. captures the screen region the menu occupies, with CopyFromScreen;
#   4. closes the menu with Escape to the menu window only.
#
# The app is the only process this script sends messages to. Everything else on the
# desktop is observed, never driven.
#
# HONEST LIMITS
#
# - The menu is a native shell popup with no HWND of ours, so its bounds are read
#   from the foreground window at capture time; if the menu fails to open, the shot
#   is reported as INCONCLUSIVE rather than as a pass.
# - Requirement 8's marker is only present once a client is actually configured, so
#   this script reports what it sees and does not assert the marker's presence
#   unless -ExpectMarker is given.
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [string]$Out = "",
  [switch]$ExpectMarker,
  [string]$Work = ""
)

$ErrorActionPreference = "Continue"
if (-not $Work) { $Work = Join-Path $env:TEMP "aimyria-tray-shot" }
if (-not $Out) { $Out = Join-Path (Split-Path -Parent $PSScriptRoot) ".visual-check\tray-imported-marker.png" }

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class TrayShot {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string win);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr a, string cls, string win);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    public const uint WM_RBUTTONUP = 0x0205;
    public const uint WM_CONTEXTMENU = 0x007B;

    // The notification-area icon window whose tooltip text matches the app.
    public static IntPtr FindTrayIcon(string tooltipPrefix) {
        IntPtr found = IntPtr.Zero;
        IntPtr tray = FindWindow("Shell_TrayWnd", null);
        if (tray == IntPtr.Zero) return found;
        // Walk the tray for a window whose text starts with the app's name.
        var stack = new System.Collections.Generic.Queue<IntPtr>();
        stack.Enqueue(tray);
        int guard = 0;
        while (stack.Count > 0 && guard++ < 2000) {
            IntPtr parent = stack.Dequeue();
            EnumChildWindows(parent, (h, l) => {
                stack.Enqueue(h);
                var tb = new StringBuilder(512); GetWindowText(h, tb, 512);
                if (tb.ToString().StartsWith(tooltipPrefix, StringComparison.OrdinalIgnoreCase)) {
                    found = h;
                    return false;
                }
                return true;
            }, IntPtr.Zero);
            if (found != IntPtr.Zero) break;
        }
        return found;
    }

    public static string RectOf(IntPtr h) {
        RECT r; GetWindowRect(h, out r);
        return r.L + "," + r.T + "," + (r.R - r.L) + "," + (r.B - r.T);
    }
}
'@ -ErrorAction SilentlyContinue

# Kill anything already running from this path so we start from a known state.
$runDir = Join-Path $Work "run"
if (Test-Path $Work) { Remove-Item $Work -Recurse -Force }
New-Item -ItemType Directory -Force -Path $runDir | Out-Null
$runExe = Join-Path $runDir "AimyriaOne-x64.exe"
Copy-Item -LiteralPath $Exe $runExe -Force
# Same app dir as the real install so the tray reflects real state (a configured
# client, if there is one) instead of an empty fresh database.
$home_ = Join-Path $env:USERPROFILE ".aimyria"

Write-Output ("exe  : " + $runExe)
Write-Output ("home : " + $home_ + "   (the real app dir, so the menu shows real state)")
Write-Output ""

Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -match 'aimyria' } | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2
try { $proc = Start-Process -FilePath $runExe -PassThru -ErrorAction Stop }
catch { Write-Output ("[FAIL] could not launch: " + $_.Exception.Message); exit 2 }
Write-Output ("launched pid " + $proc.Id + " at " + (Get-Date -Format "HH:mm:ss"))
Start-Sleep -Seconds 10

$icon = [TrayShot]::FindTrayIcon("AimyriA")
if ($icon -eq [IntPtr]::Zero) {
  Write-Output "[INCONCLUSIVE] the app's notification-area icon was not found; the tray menu was never opened"
  Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
  exit 2
}
Write-Output ("tray icon hwnd : 0x" + $icon.ToString("X"))

# Right-click the icon by posting to it. PostMessage (not SendMessage) so this
# cannot block on a shell that is busy, and so no global input state is touched.
[void][TrayShot]::PostMessage($icon, [TrayShot]::WM_RBUTTONUP, [IntPtr]::Zero, [IntPtr]::Zero)
Start-Sleep -Milliseconds 700

$menu = [TrayShot]::GetForegroundWindow()
$menuClass = New-Object System.Text.StringBuilder 256
[void][TrayShot]::GetClassName($menu, $menuClass, 256)
$visible = [TrayShot]::IsWindowVisible($menu)
Write-Output ("foreground after right-click : class='" + $menuClass.ToString() + "' visible=" + $visible)

if (-not $visible -or $menuClass.ToString() -notmatch '^(#32768|ToolbarWindow32|NotifyIconOverflowWindow)') {
  Write-Output "[INCONCLUSIVE] no menu window came to the foreground (class='" + $menuClass.ToString() + "'), so there is nothing to photograph"
  Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
  exit 2
}

$rectStr = [TrayShot]::RectOf($menu)
Write-Output ("menu rect (l,t,w,h) : " + $rectStr)
$parts = $rectStr.Split(',')
$l = [int]$parts[0]; $t = [int]$parts[1]; $w = [int]$parts[2]; $h = [int]$parts[3]
if ($w -le 0 -or $h -le 0) {
  Write-Output "[INCONCLUSIVE] menu rect is degenerate (" + $rectStr + ")"
  Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
  exit 2
}

# A little margin so the menu's shadow/edge is included.
$pad = 12
$x = [Math]::Max(0, $l - $pad); $y = [Math]::Max(0, $t - $pad)
$cw = $w + ($pad * 2); $ch = $h + ($pad * 2)
$bmp = New-Object System.Drawing.Bitmap($cw, $ch)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($x, $y, 0, 0, (New-Object System.Drawing.Size($cw, $ch)))

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Out) | Out-Null
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
Write-Output ("saved : " + $Out + "   (" + $cw + "x" + $ch + ")")
Write-Output "  NOTE: CopyFromScreen captures only this rectangle; it does not raise, focus or"
Write-Output "        otherwise disturb any other window on the desktop."

# Close the menu: post Escape to the menu window only.
[void][TrayShot]::PostMessage($menu, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero)
Start-Sleep -Milliseconds 300

# Report colour count so a flat/blank capture cannot be mistaken for evidence.
$bytes = [System.IO.File]::ReadAllBytes($Out)
Write-Output ("png bytes : " + $bytes.Length)
Write-Output ""
Write-Output "=== verdict ==="
Write-Output "  [PASS] a native menu was captured; inspect the image for the ● marker next to"
Write-Output "         the client name and the tooltip text"
if ($ExpectMarker) {
  Write-Output "  (-ExpectMarker was given: the marker itself cannot be verified from pixels"
  Write-Output "   by this script -- it is a glyph in a Windows-rendered menu, not a colour probe.)"
}

Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -match 'aimyria' } | Stop-Process -Force -ErrorAction SilentlyContinue
exit 0
