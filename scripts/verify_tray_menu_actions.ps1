# Tray menu acceptance: clicking an entry must actually DO something.
#
# WHY THIS EXISTS
#
# The tray menu shipped with an empty `.on_menu_event(|app, event| {})`, so every
# entry -- "open official website", "open main window", "switch provider", "quit"
# -- did nothing. Nothing failed: the menu built, the rows rendered, and a click
# went nowhere. A check that only asserts "the menu contains these rows" passes on
# that build, which is exactly how it survived.
#
# So this does not inspect the menu; it invokes entries and asserts the effect:
#
#   open_website -> a browser process appears after the click
#   show_main    -> the app's own top-level window becomes visible
#
# HOW IT CLICKS WITHOUT TOUCHING THE POINTER
#
# The menu is a native shell popup (#32768). Windows UI Automation can enumerate
# its items by name and Invoke one -- the same path a screen reader uses. That
# means no pointer movement and no input sent to any other application, which is
# the discipline scripts/shot_tray_menu.ps1 documents (a previous run of this
# project clicked a live button in someone else's session by moving the mouse).
#
# This file is ASCII-only on purpose: Windows PowerShell 5.1 decodes a script with
# no BOM using the system ANSI codepage (GBK here), so non-ASCII text in the source
# corrupts the parse. Menu items are matched by ASCII substrings of their real text.
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [string]$SiteHost = "aimyria.com",
  [int]$SettleSeconds = 10
)

$ErrorActionPreference = 'Continue'
$passed = 0
$failed = 0
function Ok($m) { Write-Output ("  [PASS] " + $m); $script:passed++ }
function Bad($m) { Write-Output ("  [FAIL] " + $m); $script:failed++ }
function Info($m) { Write-Output ("  " + $m) }

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class TrayIcon {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string w);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    public const uint WM_RBUTTONUP = 0x0205;

    public static IntPtr Find(string tooltipPrefix) {
        IntPtr tray = FindWindow("Shell_TrayWnd", null);
        if (tray == IntPtr.Zero) return IntPtr.Zero;
        IntPtr found = IntPtr.Zero;
        var stack = new Queue<IntPtr>(); stack.Enqueue(tray);
        int guard = 0;
        while (stack.Count > 0 && guard++ < 3000) {
            IntPtr parent = stack.Dequeue();
            EnumChildWindows(parent, (h, l) => {
                stack.Enqueue(h);
                var tb = new StringBuilder(512); GetWindowText(h, tb, 512);
                if (tb.ToString().StartsWith(tooltipPrefix, StringComparison.OrdinalIgnoreCase)) { found = h; return false; }
                return true;
            }, IntPtr.Zero);
            if (found != IntPtr.Zero) break;
        }
        return found;
    }
    public static void OpenMenu(IntPtr icon) { PostMessage(icon, WM_RBUTTONUP, IntPtr.Zero, IntPtr.Zero); }
}
'@ -ErrorAction SilentlyContinue

function Get-MenuItems {
  $root = [System.Windows.Automation.AutomationElement]::RootElement
  $cond = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::ClassNameProperty, "#32768")
  $popup = $root.FindFirst([System.Windows.Automation.TreeScope]::Children, $cond)
  if ($null -eq $popup) { return @() }
  $found = $popup.FindAll([System.Windows.Automation.TreeScope]::Descendants,
    [System.Windows.Automation.Condition]::TrueCondition)
  $out = @()
  foreach ($el in $found) { $out += ,$el }
  return $out
}

function Invoke-MenuItem($item) {
  $pattern = $null
  if ($item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
    $pattern.Invoke(); return $true
  }
  return $false
}

Write-Output ("exe : " + $Exe)
Write-Output ""

Get-Process -Name AimyriaOne -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 3
$proc = Start-Process -FilePath $Exe -PassThru
$appPid = $proc.Id
Info ("launched pid " + $appPid)
Start-Sleep -Seconds $SettleSeconds

$icon = [TrayIcon]::Find("AimyriA")
if ($icon -eq [IntPtr]::Zero) {
  Write-Output "[INCONCLUSIVE] the notification-area icon was not found; nothing could be clicked"
  Stop-Process -Id $appPid -Force -ErrorAction SilentlyContinue
  exit 2
}

$menuActionWorked = $false

# -- open the menu and list what is in it ------------------------------------
[TrayIcon]::OpenMenu($icon)
Start-Sleep -Milliseconds 1000
$items = Get-MenuItems
if ($items.Count -eq 0) {
  Write-Output "[INCONCLUSIVE] the native tray menu did not expose items to UI Automation"
  Stop-Process -Id $appPid -Force -ErrorAction SilentlyContinue
  exit 2
}
$names = @($items | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
Info ("menu opened with " + $names.Count + " named element(s):")
foreach ($n in $names) { Info ("   item: " + $n) }

# -- open_website ------------------------------------------------------------
$websiteItem = $items | Where-Object { $_.Current.Name -match ($SiteHost -replace '\.', '\.') } | Select-Object -First 1
if ($null -eq $websiteItem) {
  Info "no menu item mentions the site host; open_website cannot be verified automatically here"
} else {
  $before = @{}
  foreach ($p in Get-Process) { $before[$p.Id] = 1 }
  if (Invoke-MenuItem $websiteItem) {
    Start-Sleep -Seconds 5
    $browserish = @('chrome','msedge','firefox','brave','opera','iexplore','vivaldi','msedgewebview2')
    $hit = $null
    foreach ($p in Get-Process) {
      if ($before.ContainsKey($p.Id)) { continue }
      if ($browserish -contains $p.ProcessName.ToLower()) { $hit = $p.ProcessName; break }
    }
    if ($hit) { Ok ("open_website launched a browser: " + $hit); $menuActionWorked = $true }
    else { Bad "open_website did not launch a browser after the click" }
  } else {
    Bad "the website menu item exposed no Invoke pattern"
  }
}

# -- show_main ---------------------------------------------------------------
[TrayIcon]::OpenMenu($icon)
Start-Sleep -Milliseconds 1000
$items2 = Get-MenuItems
$showItem = $items2 | Where-Object { $_.Current.Name -match 'AimyriA go|Aimyria|main' } | Select-Object -First 1
if ($null -ne $showItem) {
  if (Invoke-MenuItem $showItem) {
    Start-Sleep -Seconds 3
    $appWindowCond = New-Object System.Windows.Automation.PropertyCondition(
      [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $appPid)
    $win = [System.Windows.Automation.AutomationElement]::RootElement.FindFirst(
      [System.Windows.Automation.TreeScope]::Children, $appWindowCond)
    if ($null -ne $win) { Ok "show_main surfaced the app window"; $menuActionWorked = $true }
    else { Bad "show_main did not surface the app window" }
  } else { Info "show-main item exposed no Invoke pattern" }
} else {
  Info "no show-main item matched by text"
}

Write-Output ""
Write-Output "=== verdict ==="
if ($menuActionWorked) { Ok "a tray entry performed its action when invoked" }
else { Bad "no tray entry produced an observable effect" }
Write-Output ("  passed=" + $passed + " failed=" + $failed)

Get-Process -Id $appPid -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
exit $failed
