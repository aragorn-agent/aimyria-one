# Full lifecycle acceptance: uninstall, reinstall, and every launch in between
# must be silent.
#
# WHAT THIS MEASURES, AND WHY IT CANNOT BE A SINGLE CHECK
#
# The owner's requirement is about the *whole* lifecycle, not just a cold start:
# uninstalling, reinstalling and the launch the installer performs all put the app
# on screen, and each is a separate chance for a console window to flash. So the
# machine's process table is diffed against the app's own process tree at every
# phase, and the verdict is the union across phases.
#
# Parent attribution rather than a bare diff: this script runs under a shell that
# itself starts conhost, so a machine-wide diff would report the harness's own
# children and cry wolf. Only processes whose ancestry reaches a launched pid
# count as the app's doing.
param(
  [Parameter(Mandatory = $true)][string]$Repo,
  [Parameter(Mandatory = $true)][string]$Installer,
  [Parameter(Mandatory = $true)][string]$Uninstaller,
  [Parameter(Mandatory = $true)][string]$Exe,
  [string]$BaseUrl = "http://127.0.0.1:8799"
)

$ErrorActionPreference = 'Continue'
$failed = 0
$consoleNames = @('reg','conhost','openconsole','cmd','where','powershell','pwsh','hostname','wscript','cscript','mshta')

$parents = @{}
function Refresh-Parents {
  $script:parents = @{}
  Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | ForEach-Object { $script:parents[[int]$_.ProcessId] = [int]$_.ParentProcessId }
}
function Get-Ancestry($p) { $c=@(); $cur=$p; for ($i=0;$i -lt 14;$i++){ if(-not $parents.ContainsKey($cur)){break}; $cur=$parents[$cur]; if($cur -le 0){break}; $c+=$cur }; return $c }

# Watch a phase: snapshot the pids, run $Action, then report every new process that
# traces back to any pid the action launched.
function Invoke-Phase($name, $Action) {
  Refresh-Parents
  $before = @{}; foreach ($p in Get-Process) { $before[$p.Id] = 1 }
  Write-Output ("--- phase: " + $name)
  $t0 = Get-Date
  $result = & $Action
  $roots = @($result | Where-Object { $_ -and $_ -ne 0 })

  $seen = @{}
  $end = (Get-Date).AddSeconds(14)
  while ((Get-Date) -lt $end) {
    foreach ($p in Get-Process) { if (-not $before.ContainsKey($p.Id)) { $seen[$p.Id] = 1 } }
    Start-Sleep -Milliseconds 40
  }
  Refresh-Parents

  $owned = @()
  foreach ($id in $seen.Keys) {
    $chain = Get-Ancestry $id
    $isOwned = $false
    foreach ($r in $roots) { if ($chain -contains [int]$r) { $isOwned = $true } }
    if ($isOwned) {
      $p = Get-Process -Id $id -ErrorAction SilentlyContinue
      if ($p) { $owned += $p.ProcessName }
    }
  }
  $console = @($owned | Where-Object { $consoleNames -contains $_.ToLower() })
  if ($console.Count -eq 0) {
    Write-Output ("  [PASS] " + $name + " started no console-hosting child of the app")
  } else {
    Write-Output ("  [FAIL] " + $name + " started console children: " + (($console | Sort-Object -Unique) -join ', '))
    $script:failed++
  }
  Write-Output ("  (app-owned new processes: " + (($owned | Sort-Object -Unique) -join ', ') + ")")
}

Get-Process -Name AimyriaOne -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 4

# 1. Uninstall (the real script, as 设置→应用 would call it)
Invoke-Phase "uninstall" {
  $p = Start-Process -FilePath 'powershell' -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',$Uninstaller,'-Uninstall') -PassThru -WindowStyle Hidden
  $p.WaitForExit(60000) | Out-Null
  @($p.Id)
}
# A "silent" uninstall that did not actually uninstall proves nothing, so the
# removal is asserted rather than assumed. The earlier version of this script
# passed while the directory was still there, because the uninstaller path was
# wrong and the phase had quietly done nothing.
$gone = -not (Test-Path -LiteralPath $Exe)
if ($gone) { Write-Output "  [ok] exe removed by uninstall" }
else { Write-Output "  [FAIL] exe still present after uninstall -- this phase proved nothing"; $failed++ }

# 2. Reinstall from the local mirror (the installer's own launch counts too)
$q = Join-Path $Repo 'scripts\quickstart.ps1'
Invoke-Phase "reinstall" {
  $p = Start-Process -FilePath 'powershell' -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',$q,'-BaseUrl',$BaseUrl) -PassThru -WindowStyle Hidden
  $p.WaitForExit(120000) | Out-Null
  # Only the app the installer launched is a root. The installer itself is the
  # user's own (hidden) shell; counting it would report its conhost as the app's
  # doing and make the verdict meaningless.
  Start-Sleep -Seconds 2
  $app = Get-Process -Name AimyriaOne -ErrorAction SilentlyContinue
  @($app.Id)
}
Write-Output ("  exe present after reinstall: " + (Test-Path -LiteralPath $Exe))
if (Test-Path -LiteralPath $Exe) {
  $hash = (Get-FileHash -LiteralPath $Exe -Algorithm SHA256).Hash.ToLower()
  Write-Output ("  installed hash: " + $hash)
  # The reinstall must have put back the build under test, not the one the public
  # site still serves -- otherwise this phase validates a binary we did not fix.
  $expected = (Get-FileHash -LiteralPath $Installer -Algorithm SHA256).Hash.ToLower()
  if ($hash -eq $expected) { Write-Output "  [ok] reinstalled bytes match the staged build under test" }
  else { Write-Output ("  [FAIL] reinstalled hash differs from staged build (" + $expected + ")"); $failed++ }
} else {
  Write-Output "  [FAIL] reinstall did not produce the exe"
  $failed++
}

Write-Output ""
Write-Output "=== verdict ==="
if ($failed -eq 0) { Write-Output "  [PASS] uninstall + reinstall lifecycle was silent throughout" }
else { Write-Output ("  [FAIL] " + $failed + " phase(s) flashed a console window") }
exit $failed
