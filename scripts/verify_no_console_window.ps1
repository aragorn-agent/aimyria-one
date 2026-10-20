# Cold-start acceptance: does ANY child process appear that would put a window on screen?
#
# WHY THIS IS A PROCESS DIFF AND NOT A WINDOW ENUMERATION
#
# verify_silent_no_flash.ps1 enumerates this process's own HWNDs. That is the right
# check for "did the app open its own UI unasked", and it passed -- while the owner
# still saw windows. The reason: the windows belonged to *other processes* the app
# started. Under a GUI binary, `reg add` gets a fresh console window (a `conhost`
# owning an `OpenConsole`), and enumerating only this pid's HWNDs can never see it.
#
# So the deciding evidence is a diff of the machine's process table across the
# launch, filtered to the classes that carry a console: reg/conhost/OpenConsole/
# cmd/where/taskkill/wsl/powershell. Zero of those after the diff is the claim.
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [int]$Seconds = 20
)

$ErrorActionPreference = 'Continue'
$exeBase = [System.IO.Path]::GetFileNameWithoutExtension($Exe)

# Cold state: a live instance absorbs the launch via the single-instance plugin,
# so we would be watching the wrong process entirely.
Get-Process -Name $exeBase -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 3
# Let the previous run's reg/conhost children finish dying, or the baseline is noisy.
Start-Sleep -Seconds 2

$before = @{}
foreach ($p in Get-Process) { $before[$p.Id] = $p.ProcessName }

$t0 = Get-Date
Start-Process -FilePath $Exe | Out-Null
Write-Output ("launched " + $exeBase + " at " + $t0.ToString('HH:mm:ss.fff'))

$consoleNames = @('reg','conhost','openconsole','cmd','where','taskkill','wsl','powershell','pwsh','hostname')
$newConsole = @()
$newOther = @()
$seen = @{}
$deadline = $t0.AddSeconds($Seconds)

while ((Get-Date) -lt $deadline) {
  foreach ($p in Get-Process) {
    if ($before.ContainsKey($p.Id)) { continue }
    if ($seen.ContainsKey($p.Id)) { continue }
    $seen[$p.Id] = 1
    $elapsed = [int]((Get-Date) - $t0).TotalMilliseconds
    $name = $p.ProcessName
    $hasWin = if ($p.MainWindowHandle -ne 0) { 'HAS-WINDOW' } else { 'no-window ' }
    $line = ('+{0,6}ms  {1,-24} pid={2,-7} {3} title="{4}"' -f $elapsed, $name, $p.Id, $hasWin, $p.MainWindowTitle)
    if ($consoleNames -contains $name.ToLower()) { $newConsole += $line }
    else { $newOther += $line }
  }
  Start-Sleep -Milliseconds 40
}

Write-Output ""
Write-Output "=== new console-hosting processes (these are the windows) ==="
if ($newConsole.Count -eq 0) {
  Write-Output "  (none)"
} else {
  $newConsole | ForEach-Object { Write-Output ("  " + $_) }
}

Write-Output ""
Write-Output "=== other new processes (webserver/webview expected) ==="
$newOther | ForEach-Object { Write-Output ("  " + $_) }

Write-Output ""
Write-Output "=== verdict ==="
if ($newConsole.Count -eq 0) {
  Write-Output "  [PASS] no console-hosting child process was started; nothing could have flashed on screen"
} else {
  Write-Output ("  [FAIL] " + $newConsole.Count + " console-hosting child process(es) started -- each is a window")
}
if (($newOther | Where-Object { $_ -match 'HAS-WINDOW' }).Count -gt 0) {
  Write-Output "  [WARN] a newly started non-console process owns a window; inspect the list above"
}

Stop-Process -Name $exeBase -Force -ErrorAction SilentlyContinue
