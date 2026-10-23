param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [switch]$ExpectQuitOnClose
)

# Verify the close behaviour matches the saved setting.
#
# The behaviour is *configurable*, and that is deliberate: a user who wants the X
# button to quit should get that. So this script reads the setting and asserts the
# matching outcome instead of assuming:
#
#   minimizeToTrayOnClose = true  -> WM_CLOSE hides the window, process survives
#   minimizeToTrayOnClose = false -> WM_CLOSE ends the process (the user's choice)
#
# The regression this guards is neither of those behaviours: it is the *silent*
# version, where the setting said "minimize" and the app quit anyway.

$ErrorActionPreference = 'Stop'

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Closer {
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr hWnd, uint msg, IntPtr w, IntPtr l);
}
"@

$settingsPath = Join-Path $env:USERPROFILE '.aimyria\settings.json'
$settings = Get-Content $settingsPath -Raw -Encoding UTF8 | ConvertFrom-Json
$minimize = $settings.minimizeToTrayOnClose
Write-Output "setting minimizeToTrayOnClose = $minimize"
if ($ExpectQuitOnClose) { $minimize = $false; Write-Output '(overridden to expect quit)' }

$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$valueName = 'AIMYRIA GO'
$saved = $null
try { $saved = (Get-ItemProperty -Path $runKey -Name $valueName -ErrorAction Stop).$valueName } catch { }

$proc = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds 3
if ($proc.HasExited) { Write-Output 'FAIL: died on start'; exit 1 }

$second = Start-Process -FilePath $Exe -PassThru
$second.WaitForExit(5000) | Out-Null
Start-Sleep -Seconds 2

$proc.Refresh()
$main = (Get-Process -Id $proc.Id).MainWindowHandle
Write-Output "pid=$($proc.Id) hwnd=$main"

[Closer]::PostMessageW([IntPtr]$main, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Start-Sleep -Seconds 3
$proc.Refresh()
$alive = -not $proc.HasExited
Write-Output "alive after WM_CLOSE = $alive"

$nowRun = $null
try { $nowRun = (Get-ItemProperty -Path $runKey -Name $valueName -ErrorAction Stop).$valueName } catch { }
if ($saved -and $nowRun -ne $saved) {
    Set-ItemProperty -Path $runKey -Name $valueName -Value $saved
}

if ($minimize) {
    if ($alive) {
        Write-Output 'RESULT: PASS — setting says minimize; the process survived'
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
        exit 0
    }
    Write-Output 'RESULT: FAIL — setting says minimize but the process died'
    exit 1
} else {
    if (-not $alive) {
        Write-Output 'RESULT: PASS — setting says quit; the process ended'
        exit 0
    }
    Write-Output 'RESULT: FAIL — setting says quit but the process survived'
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    exit 1
}
