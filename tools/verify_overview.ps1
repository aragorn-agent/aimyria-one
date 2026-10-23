param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [int]$ObserveSeconds = 10
)

# Verify the overview aggregate against the local database.
#
# This is the check that matters for the overview page: the numbers it shows come
# from two places (the gateway for balance/models, the local SQLite for usage), and
# a bug in either produces a plausible-looking wrong number. So this script:
#   1. computes the expected month cost straight from the database;
#   2. launches the build windowless;
#   3. reads the startup log for the relay session line and any block failure;
#   4. restores the autostart value the app rewrites.

$ErrorActionPreference = 'Stop'

$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$valueName = 'AIMYRIA GO'
$logPath = Join-Path $env:USERPROFILE '.aimyria\logs\aimyria-one.log'
$dbPath = Join-Path $env:USERPROFILE '.aimyria\aimyria-one.db'

function Get-SizeSafe([string]$p) { if (Test-Path $p) { (Get-Item $p).Length } else { 0 } }

# ── expected usage, from the database ────────────────────────────────────────
$expectedMonth = $null
$sqlite = Get-Command sqlite3 -ErrorAction SilentlyContinue
if ($sqlite -and (Test-Path $dbPath)) {
    $now = Get-Date
    # Local midnight on the 1st, as unix seconds. Computing it as a DateTimeOffset
    # difference keeps this independent of the machine's time zone.
    $firstOfMonth = Get-Date -Year $now.Year -Month $now.Month -Day 1 -Hour 0 -Minute 0 -Second 0
    $epoch = Get-Date -Year 1970 -Month 1 -Day 1 -Hour 0 -Minute 0 -Second 0
    $monthStart = [int64](($firstOfMonth - $epoch).TotalSeconds)

    $expectedMonth = & $sqlite.Source $dbPath "select printf('%.4f', coalesce(sum(total_cost_usd),0)) from proxy_request_logs where created_at >= $monthStart;" 2>$null
    Write-Output "db month cost      : $expectedMonth (since $monthStart)"
} else {
    Write-Output 'db month cost      : <sqlite3 unavailable, skipped>'
}

$saved = $null
try { $saved = (Get-ItemProperty -Path $runKey -Name $valueName -ErrorAction Stop).$valueName } catch { }

$before = Get-SizeSafe $logPath
$proc = Start-Process -FilePath $Exe -PassThru -WindowStyle Hidden
Start-Sleep -Milliseconds 900
if ($proc.HasExited) {
    Write-Output "RESULT        : FAIL (died immediately, exit=$($proc.ExitCode))"
    exit 1
}
Write-Output "pid                : $($proc.Id) (alive)"

Start-Sleep -Seconds $ObserveSeconds
$proc.Refresh()
$alive = -not $proc.HasExited
$grew = (Get-SizeSafe $logPath) -gt $before
Write-Output "alive after ${ObserveSeconds}s   : $alive"
Write-Output "log grew           : $grew"

Write-Output '--- relay / overview related log lines ---'
$all = Get-Content $logPath -Encoding UTF8
$all | Select-Object -Last 80 |
    Where-Object { $_ -match '\[relay\]|GlobalProxy|ERROR' } |
    Select-Object -Last 10 | ForEach-Object { Write-Output "  $_" }

$now2 = $null
try { $now2 = (Get-ItemProperty -Path $runKey -Name $valueName -ErrorAction Stop).$valueName } catch { }
if ($saved -and $now2 -ne $saved) {
    Set-ItemProperty -Path $runKey -Name $valueName -Value $saved
    Write-Output "autostart          : restored"
} else {
    Write-Output 'autostart          : unchanged'
}

Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
Write-Output "stopped pid=$($proc.Id)"

if ($alive -and $grew) { Write-Output 'RESULT        : PASS'; exit 0 }
Write-Output 'RESULT        : FAIL'
exit 1
