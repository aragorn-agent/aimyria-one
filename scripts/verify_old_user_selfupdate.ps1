# Old-user upgrade acceptance: does a machine already on the retired build get the
# fix by itself, silently?
#
# This is the leg everything else leaves unproven. Publishing v1.0.3 only helps
# people who download it fresh; everyone already running v1.0.2 depends entirely
# on the silent self-updater, and the whole point of the fix is that they never
# see anything happen. So: install the retired v1.0.2 bytes, let the app update
# itself against the real public manifest, and watch both the window behaviour
# and the installed hash.
#
# The test clock shortens the schedule (first check 60s -> 3s, replace pause
# 15s -> 0.5s). Only the timings change; the allowlist, the sha256 gate and the
# rename are the shipping code paths.
param(
  [Parameter(Mandatory = $true)][string]$OldExe,
  [Parameter(Mandatory = $true)][string]$InstallDir,
  [int]$TimeoutSeconds = 150
)

$ErrorActionPreference = 'Continue'
$target = Join-Path $InstallDir 'AimyriaOne.exe'
$consoleNames = @('reg','conhost','openconsole','cmd','where','powershell','pwsh','hostname','taskkill')

Get-Process -Name AimyriaOne -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 4

# Put the machine back on the retired build -- this is "a user who installed before".
Copy-Item -LiteralPath $OldExe -Destination $target -Force
$oldSha = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLower()
$oldVer = '1.0.2'
Write-Output ("start state : v1.0.2  sha256=" + $oldSha)

# Snapshot the parent chain of the launched process so its children can be
# identified, and sample every new process while the update runs.
$parents = @{}
function Refresh-Parents {
  $script:parents = @{}
  Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
    ForEach-Object { $script:parents[[int]$_.ProcessId] = [int]$_.ParentProcessId }
}
function Get-Ancestry($p) { $c=@(); $cur=$p; for ($i=0;$i -lt 14;$i++){ if(-not $parents.ContainsKey($cur)){break}; $cur=$parents[$cur]; if($cur -le 0){break}; $c+=$cur }; return $c }

Refresh-Parents
$before = @{}; foreach ($p in Get-Process) { $before[$p.Id] = 1 }

# Launch with the test clock, exactly as an acceptance run would. `-Environment`
# is PowerShell 7 only, so the variable is set on this shell and inherited; the
# shipping timings are the only thing it changes.
$env:AIMYRIA_UPDATE_TEST_CLOCK = '1'
$proc = Start-Process -FilePath $target -PassThru
$rootPid = $proc.Id
$t0 = Get-Date
Write-Output ("launched pid " + $rootPid + " at " + $t0.ToString('HH:mm:ss.fff'))

$seen = @{}
$oldVer = '1.0.2'
$finalVer = $oldVer
$deadline = $t0.AddSeconds($TimeoutSeconds)
while ((Get-Date) -lt $deadline) {
  foreach ($p in Get-Process) { if (-not $before.ContainsKey($p.Id)) { $seen[$p.Id] = 1 } }
  # Read the version without blocking the updater's rename. `Get-FileHash` opens
  # with a share mode that omits FILE_SHARE_DELETE, and a held handle makes the
  # updater's `rename exe -> exe.old` fail with os error 32 -- the updater would
  # then defer to its next cycle for a reason this harness created. So the probe
  # itself must allow delete-sharing, or it measures its own interference.
  try {
    $fs = [System.IO.File]::Open($target, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read,
                                ([System.IO.FileShare]::ReadWrite -bor [System.IO.FileShare]::Delete))
    $buf = New-Object byte[] 4096
    $null = $fs.Read($buf, 0, $buf.Length)
    $fs.Close()
    # The banner is the only trustworthy version marker (dependency versions
    # litter the binary, so grepping "1.0.x" finds the wrong ones).
    $head = [System.Text.Encoding]::ASCII.GetString($buf)
    if ($head -match 'AimyriA go v(\d+\.\d+\.\d+)') {
      $ver = $matches[1]
      if ($ver -ne $oldVer) {
        $finalVer = $ver
        Write-Output ("  exe replaced at +" + [int]((Get-Date) - $t0).TotalMilliseconds + "ms -> banner v" + $ver)
        break
      }
    }
  } catch { }
  Start-Sleep -Milliseconds 150
}
Refresh-Parents

Write-Output ""
Write-Output "=== new processes traceable to the launched app ==="
$owned = @()
foreach ($id in $seen.Keys) {
  $ch = Get-Ancestry $id
  if ($ch -contains $rootPid) {
    $p = Get-Process -Id $id -ErrorAction SilentlyContinue
    if ($p) { $owned += $p.ProcessName; Write-Output ("  pid=" + $id + "  " + $p.ProcessName) }
  }
}
if ($owned.Count -eq 0) { Write-Output "  (none)" }

Write-Output ""
Write-Output "=== verdict ==="
$finalSha = (Get-FileHash -LiteralPath $target -Algorithm SHA256 -ErrorAction SilentlyContinue).Hash
if ($finalSha) { $finalSha = $finalSha.ToLower() }
$expected = 'd9834365b20540c58d46ff8b8ee0c159b249f9f8c375081ae0036cbfeaabaaf9'
if ($finalSha -eq $expected) {
  Write-Output "  [PASS] the app updated itself to v1.0.3 (the fixed build) with no user action"
} else {
  Write-Output ("  [FAIL] exe did not become v1.0.3; now " + $finalSha)
}
$console = @($owned | Where-Object { $consoleNames -contains $_.ToLower() })
if ($console.Count -eq 0) {
  Write-Output "  [PASS] the upgrade started no console-hosting child of the app"
} else {
  Write-Output ("  [FAIL] console children during upgrade: " + (($console | Sort-Object -Unique) -join ', '))
}

# Report the updater's own narration -- the log is the only witness to the
# phases, and PASS on the hash alone would not show it was silent on the way.
$log = Join-Path $env:USERPROFILE '.aimyria\logs\aimyria-one.log'
Write-Output ""
Write-Output "=== updater log lines from this run ==="
Get-Content -LiteralPath $log -Tail 60 -ErrorAction SilentlyContinue |
  Where-Object { $_ -match '\[updater\]' } | Select-Object -Last 14 | ForEach-Object { Write-Output ("  " + $_) }
