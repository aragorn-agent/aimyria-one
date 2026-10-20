# Attribute every new process to a parent, so "the app started it" is provable.
#
# The machine-wide diff in verify_no_console_window.ps1 cannot tell the app's
# children from the test harness's own siblings (the harness runs under a shell
# that itself starts conhost). This walks ParentProcessId instead: anything whose
# ancestry reaches the launched pid is the app's doing; everything else is noise.
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [int]$Seconds = 18
)

$ErrorActionPreference = 'Continue'
$exeBase = [System.IO.Path]::GetFileNameWithoutExtension($Exe)

Get-Process -Name $exeBase -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 3
Start-Sleep -Seconds 2

$before = @{}
foreach ($p in Get-Process) { $before[$p.Id] = 1 }

$t0 = Get-Date
$proc = Start-Process -FilePath $Exe -PassThru
$rootPid = $proc.Id
Write-Output ("launched pid " + $rootPid + " at " + $t0.ToString('HH:mm:ss.fff'))

$seen = @{}
$deadline = $t0.AddSeconds($Seconds)
while ((Get-Date) -lt $deadline) {
  foreach ($p in Get-Process) {
    if ($before.ContainsKey($p.Id)) { continue }
    $seen[$p.Id] = 1
  }
  Start-Sleep -Milliseconds 40
}

# Build pid -> parent from CIM once, then resolve ancestry against rootPid.
$parent = @{}
try {
  Get-CimInstance Win32_Process -ErrorAction Stop | ForEach-Object { $parent[[int]$_.ProcessId] = [int]$_.ParentProcessId }
} catch {
  Write-Output ("  (CIM unavailable: " + $_.Exception.Message + ")")
}

function Get-Ancestry($pid_) {
  $chain = @()
  $cur = $pid_
  for ($i = 0; $i -lt 12; $i++) {
    if (-not $parent.ContainsKey($cur)) { break }
    $cur = $parent[$cur]
    if ($cur -le 0) { break }
    $chain += $cur
  }
  return $chain
}

Write-Output ""
Write-Output "=== new processes whose ancestry reaches the launched app ==="
$appOwned = @()
foreach ($id in $seen.Keys) {
  $chain = Get-Ancestry $id
  if ($chain -contains $rootPid) {
    $p = Get-Process -Id $id -ErrorAction SilentlyContinue
    $name = if ($p) { $p.ProcessName } else { '?' }
    $appOwned += $name
    Write-Output ("  pid=" + $id + "  " + $name)
  }
}
if ($appOwned.Count -eq 0) { Write-Output "  (none)" }

Write-Output ""
Write-Output "=== verdict ==="
$consoleOwned = @($appOwned | Where-Object { @('reg','conhost','openconsole','cmd','where','powershell','pwsh','hostname') -contains $_.ToLower() })
Write-Output ("  app-owned processes: " + $appOwned.Count + "  ->  " + (($appOwned | Sort-Object -Unique) -join ', '))
if ($consoleOwned.Count -eq 0) {
  Write-Output "  [PASS] the app started no console-hosting child; nothing of its doing can put a window on screen"
} else {
  Write-Output ("  [FAIL] the app started console children: " + (($consoleOwned | Sort-Object -Unique) -join ', '))
}

Stop-Process -Id $rootPid -Force -ErrorAction SilentlyContinue
