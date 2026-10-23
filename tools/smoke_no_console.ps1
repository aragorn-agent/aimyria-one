param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [int]$ObserveSeconds = 12
)

# Smoke test shaped exactly like the failure being fixed:
#   * launch the release binary with NO console attached (Start-Process, hidden)
#     -- this is what a double-click or the autostart entry does, and it is the
#        condition that made fern's stderr fallback panic and kill the process;
#   * confirm the process is still alive after a settling period;
#   * confirm the log file grew (logging works from a windowless process);
#   * confirm no NEW crash report appeared;
#   * restore the HKCU Run value, because the app rewrites it to point at itself
#     and this script must not leave the user's autostart aimed at a build dir.

$ErrorActionPreference = 'Stop'

$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$valueName = 'AIMYRIA GO'
$logPath = Join-Path $env:USERPROFILE '.aimyria\logs\aimyria-one.log'
$crashPath = Join-Path $env:USERPROFILE '.aimyria\crash.log'

function Get-SizeSafe([string]$p) {
    if (Test-Path $p) { return (Get-Item $p).Length } else { return 0 }
}

# --- preserve the user's autostart entry -------------------------------------
$savedRun = $null
try {
    $savedRun = (Get-ItemProperty -Path $runKey -Name $valueName -ErrorAction Stop).$valueName
} catch {
    $savedRun = $null
}

$logBefore = Get-SizeSafe $logPath
$crashBefore = Get-SizeSafe $crashPath

Write-Output "exe           : $Exe"
Write-Output "autostart was : $(if ($savedRun) { $savedRun } else { '<absent>' })"
Write-Output "log before    : $logBefore bytes"
Write-Output "crash before  : $crashBefore bytes"
Write-Output "launching windowless..."

$proc = Start-Process -FilePath $Exe -PassThru -WindowStyle Hidden
Start-Sleep -Milliseconds 900
if ($proc.HasExited) {
    Write-Output "RESULT        : FAIL (died immediately, exit=$($proc.ExitCode))"
    exit 1
}
Write-Output "pid           : $($proc.Id) (alive after 0.9s)"

Write-Output "observing for $ObserveSeconds s..."
Start-Sleep -Seconds $ObserveSeconds
$proc.Refresh()

$alive = -not $proc.HasExited
$logAfter = Get-SizeSafe $logPath
$crashAfter = Get-SizeSafe $crashPath
$logGrew = $logAfter -gt $logBefore
$newCrash = $crashAfter -gt $crashBefore

Write-Output "alive after   : ${ObserveSeconds}s = $alive"
Write-Output "log after     : $logAfter bytes (grew: $logGrew)"
Write-Output "crash after   : $crashAfter bytes (new crash: $newCrash)"

if (Test-Path $logPath) {
    Write-Output "--- log tail ---"
    Get-Content $logPath -Tail 10 -Encoding UTF8 | ForEach-Object { Write-Output "  $_" }
}

# --- restore the user's autostart entry before judging -----------------------
$nowRun = $null
try {
    $nowRun = (Get-ItemProperty -Path $runKey -Name $valueName -ErrorAction Stop).$valueName
} catch {
    $nowRun = $null
}
if ($savedRun -and $nowRun -ne $savedRun) {
    Set-ItemProperty -Path $runKey -Name $valueName -Value $savedRun
    Write-Output "autostart     : restored to $savedRun"
} elseif (-not $savedRun -and $nowRun) {
    Remove-ItemProperty -Path $runKey -Name $valueName -ErrorAction SilentlyContinue
    Write-Output "autostart     : removed (it was absent before)"
} else {
    Write-Output "autostart     : unchanged"
}

if ($alive -and $logGrew -and -not $newCrash) {
    Write-Output "RESULT        : PASS"
    if ($env:KEEP_RUNNING -ne '1') {
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
        Write-Output "cleaned up    : stopped pid $($proc.Id)"
    }
    exit 0
} else {
    Write-Output "RESULT        : FAIL"
    if ($alive) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    exit 1
}
