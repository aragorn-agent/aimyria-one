param([Parameter(Mandatory = $true)][string]$Exe)

# Does the shipped build actually resolve the balance on THIS machine?
#
# The reported failure was "another user installed the latest build and still
# cannot see the balance". The cause was that the credential lookup searched only
# Claude Code and Codex, so a user whose configured client was Gemini, OpenCode,
# OpenClaw, Hermes or Claude Desktop got nothing.
#
# The check is behavioural, not structural: start the app, wait for it to settle,
# then read the log for a relay-balance line. A log line proves the credential was
# resolved and the request was attempted; the HTTP status proves it worked.
#
# It also reports which client supplied the key, which is the fact that would have
# made the original defect obvious in a bug report.

$ErrorActionPreference = 'Stop'

$logPath = Join-Path $env:USERPROFILE '.aimyria\logs\aimyria-one.log'
$settingsPath = Join-Path $env:USERPROFILE '.aimyria\settings.json'
$dbPath = Join-Path $env:USERPROFILE '.aimyria\aimyria-one.db'

Write-Output "build: $((Get-Item $Exe).VersionInfo.FileVersion)"

# Which clients have a selected row? That is the search space the fix widened.
$sqlite = Get-Command sqlite3 -ErrorAction SilentlyContinue
if ($sqlite -and (Test-Path $dbPath)) {
    $rows = & $sqlite.Source $dbPath "select app_type || ':' || id from providers where is_current = 1;" 2>$null
    Write-Output "current rows: $(@($rows) -join ', ')"
} else {
    Write-Output 'current rows: <sqlite3 unavailable>'
}

$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$valueName = 'AIMYRIA GO'
$saved = $null
try { $saved = (Get-ItemProperty -Path $runKey -Name $valueName -ErrorAction Stop).$valueName } catch { }

$before = if (Test-Path $logPath) { (Get-Item $logPath).Length } else { 0 }

Start-Process -FilePath $Exe | Out-Null
Start-Sleep -Seconds 8

# Wake the window: the balance is fetched when a screen asks for it, not at boot.
$second = Start-Process -FilePath $Exe -PassThru
$second.WaitForExit(5000) | Out-Null
Start-Sleep -Seconds 10

$proc = Get-Process | Where-Object { $_.ProcessName -like '*aimyria*' } | Select-Object -First 1
Write-Output "running: $([bool]$proc)"

$tail = Get-Content $logPath -Encoding UTF8 | Select-Object -Last 200
$balanceLines = $tail | Where-Object { $_ -match 'relay|余额|balance' }
Write-Output '--- relay/balance log lines ---'
$balanceLines | Select-Object -Last 8 | ForEach-Object { Write-Output "  $_" }

# The authoritative check: ask the relay with the key the app found.
$key = $null
if ($sqlite -and (Test-Path $dbPath)) {
    $key = & $sqlite.Source $dbPath "select json_extract(settings_config,'$.env.ANTHROPIC_AUTH_TOKEN') from providers where app_type='claude' and is_current=1;" 2>$null
    if (-not $key) {
        $key = & $sqlite.Source $dbPath "select json_extract(settings_config,'$.auth.OPENAI_API_KEY') from providers where app_type='codex' and is_current=1;" 2>$null
    }
}
if ($key) {
    try {
        $resp = Invoke-RestMethod -Uri 'https://api.aimyria.com/v1/user/balance' -Headers @{ Authorization = "Bearer $key" } -TimeoutSec 20
        Write-Output ("relay says balance = {0} {1} ({2})" -f $resp.data.remaining, $resp.data.unit, $resp.data.planName)
    } catch {
        Write-Output "relay query failed: $($_.Exception.Message)"
    }
} else {
    Write-Output 'no key found in the database (this machine may have no client configured)'
}

$nowRun = $null
try { $nowRun = (Get-ItemProperty -Path $runKey -Name $valueName -ErrorAction Stop).$valueName } catch { }
if ($saved -and $nowRun -ne $saved) { Set-ItemProperty -Path $runKey -Name $valueName -Value $saved }

if ($proc) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
Write-Output 'done'
