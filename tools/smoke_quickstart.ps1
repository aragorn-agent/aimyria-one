param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [int]$ObserveSeconds = 10
)

# End-to-end check of the 接入中心 status bug:
#   * launch the build windowless (the real user condition);
#   * confirm it stays alive and logs;
#   * print the client-status lines the backend now emits, so "configured" can be
#     read from the log rather than guessed;
#   * restore the autostart value the app rewrites on startup.

$ErrorActionPreference = 'Stop'

$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$valueName = 'AIMYRIA GO'
$logPath = Join-Path $env:USERPROFILE '.aimyria\logs\aimyria-one.log'

function Get-SizeSafe([string]$p) { if (Test-Path $p) { (Get-Item $p).Length } else { 0 } }

$saved = $null
try { $saved = (Get-ItemProperty -Path $runKey -Name $valueName -ErrorAction Stop).$valueName } catch { }

$before = Get-SizeSafe $logPath
$proc = Start-Process -FilePath $Exe -PassThru -WindowStyle Hidden
Start-Sleep -Milliseconds 900
if ($proc.HasExited) {
    Write-Output "FAIL: died immediately (exit=$($proc.ExitCode))"
    exit 1
}
Write-Output "pid=$($proc.Id) alive"

Start-Sleep -Seconds $ObserveSeconds
$proc.Refresh()
Write-Output "alive after ${ObserveSeconds}s: $(-not $proc.HasExited)"
Write-Output "log grew: $((Get-SizeSafe $logPath) -gt $before)"

Write-Output '--- new log lines (global proxy / hermes / errors) ---'
$all = Get-Content $logPath -Encoding UTF8
$new = $all | Select-Object -Skip ([Math]::Max(0, $all.Count - 60))
$new | Where-Object { $_ -match 'GlobalProxy|Hermes|hermes|ERROR|WARN|ERROR' } | Select-Object -Last 12 | ForEach-Object { Write-Output "  $_" }

# Restore autostart if the app moved it.
$now = $null
try { $now = (Get-ItemProperty -Path $runKey -Name $valueName -ErrorAction Stop).$valueName } catch { }
if ($saved -and $now -ne $saved) {
    Set-ItemProperty -Path $runKey -Name $valueName -Value $saved
    Write-Output "autostart restored: $saved"
} else {
    Write-Output 'autostart unchanged'
}

Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
Write-Output "stopped pid=$($proc.Id)"
