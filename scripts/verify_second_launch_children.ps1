# Second-launch acceptance: is a repeat launch (single-instance forward) silent too?
#
# The first launch is the easy case. The owner's complaint also covers repeated
# starts -- a script, a start-on-login entry firing twice, the updater relaunching
# -- every one of which forward to the running instance. Each forward is a fresh
# chance for the app to start something that puts a window on screen, so it is
# measured separately and with parent attribution (a machine-wide diff cannot tell
# the app's children from this harness's own).
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [int]$SettleSeconds = 6,
  [int]$WatchSeconds = 8
)

$ErrorActionPreference = 'Continue'
$exeBase = [System.IO.Path]::GetFileNameWithoutExtension($Exe)
Get-Process -Name $exeBase -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 4

$pid1 = (Start-Process -FilePath $Exe -PassThru).Id
Write-Output ("first launch pid " + $pid1)
Start-Sleep -Seconds $SettleSeconds

$parent = @{}
Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | ForEach-Object { $parent[[int]$_.ProcessId] = [int]$_.ParentProcessId }
function Get-Ancestry($p) { $c=@(); $cur=$p; for ($i=0;$i -lt 12;$i++){ if(-not $parent.ContainsKey($cur)){break}; $cur=$parent[$cur]; if($cur -le 0){break}; $c+=$cur }; return $c }

$before = @{}; foreach ($p in Get-Process) { $before[$p.Id] = 1 }
$t1 = Get-Date
$pid2 = (Start-Process -FilePath $Exe -PassThru).Id
Write-Output ("second launch pid " + $pid2 + " at " + $t1.ToString('HH:mm:ss.fff'))

$seen = @{}
$end = $t1.AddSeconds($WatchSeconds)
while ((Get-Date) -lt $end) {
  foreach ($p in Get-Process) { if (-not $before.ContainsKey($p.Id)) { $seen[$p.Id] = 1 } }
  Start-Sleep -Milliseconds 40
}

Write-Output ""
Write-Output "=== new processes traceable to either launched pid ==="
$owned = @()
foreach ($id in $seen.Keys) {
  $chain = Get-Ancestry $id
  if (($chain -contains $pid1) -or ($chain -contains $pid2)) {
    $p = Get-Process -Id $id -ErrorAction SilentlyContinue
    $nm = if ($p) { $p.ProcessName } else { '?' }
    $owned += $nm
    Write-Output ("  pid=" + $id + "  " + $nm)
  }
}
if ($owned.Count -eq 0) { Write-Output "  (none)" }

Write-Output ""
Write-Output "=== verdict ==="
$console = @($owned | Where-Object { @('reg','conhost','openconsole','cmd','where','powershell','pwsh','hostname') -contains $_.ToLower() })
if ($console.Count -eq 0) {
  Write-Output "  [PASS] second launch started no console-hosting child of the app"
} else {
  Write-Output ("  [FAIL] app-owned console children: " + (($console | Sort-Object -Unique) -join ', '))
}

Get-Process -Name $exeBase -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
