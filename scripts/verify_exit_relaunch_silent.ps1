# Exit-and-relaunch acceptance: quitting and starting again must both be silent.
#
# The remaining lifecycle leg the other scripts do not cover. Restart in this app
# goes through `tauri::process::restart`, which spawns the exe directly (no shell),
# so the relaunch is the same code path as a cold start -- but this measures it
# rather than reasoning about it, because the point of the exercise is to stop
# trusting reasoning about window behaviour.
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [int]$Seconds = 12
)

$ErrorActionPreference = 'Continue'
$exeBase = [System.IO.Path]::GetFileNameWithoutExtension($Exe)
$consoleNames = @('reg','conhost','openconsole','cmd','where','powershell','pwsh','hostname','taskkill')

Get-Process -Name $exeBase -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 4

$parents = @{}
function Refresh { $script:parents = @{}; Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | ForEach-Object { $script:parents[[int]$_.ProcessId] = [int]$_.ParentProcessId } }
function Anc($p) { $c=@(); $cur=$p; for($i=0;$i -lt 14;$i++){ if(-not $parents.ContainsKey($cur)){break}; $cur=$parents[$cur]; if($cur -le 0){break}; $c+=$cur }; return $c }

function Watch($label, $Roots) {
  Refresh
  $before = @{}; foreach ($p in Get-Process) { $before[$p.Id] = 1 }
  $seen = @{}
  $end = (Get-Date).AddSeconds($Seconds)
  while ((Get-Date) -lt $end) {
    foreach ($p in Get-Process) { if (-not $before.ContainsKey($p.Id)) { $seen[$p.Id] = 1 } }
    Start-Sleep -Milliseconds 40
  }
  Refresh
  $owned = @()
  foreach ($id in $seen.Keys) {
    $ch = Anc $id
    foreach ($r in $Roots) { if ($r -and ($ch -contains [int]$r)) { $p = Get-Process -Id $id -EA SilentlyContinue; if ($p) { $owned += $p.ProcessName }; break } }
  }
  $console = @($owned | Where-Object { $consoleNames -contains $_.ToLower() })
  if ($console.Count -eq 0) { Write-Output ("  [PASS] " + $label + " silent") }
  else { Write-Output ("  [FAIL] " + $label + " console children: " + (($console|Sort-Object -Unique) -join ', ')) }
  Write-Output ("         app-owned: " + (($owned | Sort-Object -Unique) -join ', '))
}

# Launch, let it settle.
Refresh
$before = @{}; foreach ($p in Get-Process) { $before[$p.Id] = 1 }
$p1 = (Start-Process -FilePath $Exe -PassThru).Id
Write-Output ("launched pid " + $p1)
Start-Sleep -Seconds 6

# Quit it (as a tray "Exit"/task kill would) and watch for anything the app starts
# while tearing down.
Write-Output "quitting..."
Refresh
$beforeQ = @{}; foreach ($p in Get-Process) { $beforeQ[$p.Id] = 1 }
Get-Process -Name $exeBase -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
$qseen = @{}
$qe = (Get-Date).AddSeconds(4)
while ((Get-Date) -lt $qe) { foreach ($p in Get-Process) { if (-not $beforeQ.ContainsKey($p.Id)) { $qseen[$p.Id] = 1 } }; Start-Sleep -Milliseconds 40 }
Refresh
$qowned = @()
foreach ($id in $qseen.Keys) {
  $p = Get-Process -Id $id -EA SilentlyContinue
  if ($p -and ($consoleNames -contains $p.ProcessName.ToLower())) { $qowned += $p.ProcessName }
}
if ($qowned.Count -eq 0) { Write-Output "  [PASS] quit silent (no console process appeared during teardown)" }
else { Write-Output ("  [INFO] console processes appeared during teardown: " + (($qowned|Sort-Object -Unique) -join ', ')) }

Start-Sleep -Seconds 2
# Relaunch
Watch "relaunch" @((Start-Process -FilePath $Exe -PassThru).Id)
Start-Sleep -Seconds 1
Get-Process -Name $exeBase -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
