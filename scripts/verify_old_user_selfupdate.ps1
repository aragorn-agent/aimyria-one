# Old-user upgrade acceptance: does a machine already on an older build get the fix
# by itself, silently?
#
# This is the leg everything else leaves unproven. Publishing a build only helps
# people who download it fresh; everyone already running an older one depends
# entirely on the silent self-updater, and the whole point is that they never see
# anything happen. So: install the older bytes, let the app update itself against
# the real public manifest, and check both the window behaviour and the installed hash.
#
# WHY THE DATABASE VERSION IS WRITTEN BACK
#
# A machine being tested here has the *binary* rolled back but the *database* left
# at the current schema. An older binary refuses a newer database and drops into
# recovery mode, which shows the window on purpose -- and a visible window makes the
# updater defer, every cycle, forever. That is a harness artefact, not a user's state:
# a real old-build user has an old database. So the DB's user_version is set to the
# version the old build expects, which reproduces the real situation.
#
# WHY THE EXPECTED HASH IS A PARAMETER
#
# The first version of this script compared the result against the *old* build's
# hash, so "the exe is unchanged" satisfied the pass condition -- it reported PASS
# while the update had actually been deferred every cycle. A check that passes on
# "nothing happened" is worse than no check, so the target hash is required input
# and an unchanged exe is an explicit failure.
#
# The test clock shortens the schedule (first check 60s -> 3s). Only timings change;
# the allowlist, the sha256 gate and the rename are the shipping code paths.
param(
  [Parameter(Mandatory = $true)][string]$OldExe,
  [Parameter(Mandatory = $true)][string]$InstallDir,
  [Parameter(Mandatory = $true)][string]$ExpectedSha256,
  [int]$OldDbVersion = 0,
  [int]$TimeoutSeconds = 150
)

$ErrorActionPreference = 'Continue'
$target = Join-Path $InstallDir 'AimyriaOne.exe'
$consoleNames = @('reg','conhost','openconsole','cmd','where','powershell','pwsh','hostname','taskkill')

Get-Process -Name AimyriaOne -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 4

# Put the machine back on the older build.
Copy-Item -LiteralPath $OldExe -Destination $target -Force
$oldSha = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLower()
Write-Output ("start state : old build  sha256=" + $oldSha)

# Match the database to what the old build understands, or it enters recovery mode
# (window shown -> updater defers forever) and this measures the harness, not the user.
if ($OldDbVersion -gt 0) {
  $dbPath = Join-Path $env:USERPROFILE '.aimyria\aimyria-one.db'
  if (Test-Path -LiteralPath $dbPath) {
    $backup = "$dbPath.pre-olduser-test"
    Copy-Item -LiteralPath $dbPath -Destination $backup -Force
    try {
      $py = @"
import sqlite3
c = sqlite3.connect(r'$dbPath')
c.execute('PRAGMA user_version = $OldDbVersion')
c.commit()
print('db user_version ->', c.execute('PRAGMA user_version').fetchone()[0])
"@
      $py | python - 2>&1 | ForEach-Object { Write-Output ("  " + $_) }
      Write-Output ("  (db backup kept at " + $backup + ")")
    } catch {
      Write-Output ("  [FAIL] could not set db version: " + $_.Exception.Message)
    }
  }
}

$parents = @{}
function Refresh-Parents {
  $script:parents = @{}
  Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
    ForEach-Object { $script:parents[[int]$_.ProcessId] = [int]$_.ParentProcessId }
}
function Get-Ancestry($p) { $c=@(); $cur=$p; for ($i=0;$i -lt 14;$i++){ if(-not $parents.ContainsKey($cur)){break}; $cur=$parents[$cur]; if($cur -le 0){break}; $c+=$cur }; return $c }

Refresh-Parents
$before = @{}; foreach ($p in Get-Process) { $before[$p.Id] = 1 }

# Launch with the test clock; the shipping timings are the only thing it changes.
$env:AIMYRIA_UPDATE_TEST_CLOCK = '1'
$proc = Start-Process -FilePath $target -PassThru
$rootPid = $proc.Id
$t0 = Get-Date
Write-Output ("launched pid " + $rootPid + " at " + $t0.ToString('HH:mm:ss.fff'))

$seen = @{}
$oldVer = '1.0.0'
$finalVer = $oldVer
$finalSha = $oldSha
$deadline = $t0.AddSeconds($TimeoutSeconds)
while ((Get-Date) -lt $deadline) {
  foreach ($p in Get-Process) { if (-not $before.ContainsKey($p.Id)) { $seen[$p.Id] = 1 } }
  # Read the version without blocking the updater's rename: Get-FileHash opens
  # without FILE_SHARE_DELETE, and a held handle makes the rename fail with
  # os error 32 -- then the updater defers for a reason this harness created.
  try {
    $fs = [System.IO.File]::Open($target, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read,
                                ([System.IO.FileShare]::ReadWrite -bor [System.IO.FileShare]::Delete))
    $buf = New-Object byte[] 200000
    $null = $fs.Read($buf, 0, $buf.Length)
    $fs.Close()
    $head = [System.Text.Encoding]::ASCII.GetString($buf)
    if ($head -match '=== AimyriA go v(\d+\.\d+\.\d+) started ===') {
      $ver = $matches[1]
      if ($ver -ne $finalVer) {
        $finalVer = $ver
        Write-Output ("  exe replaced at +" + [int]((Get-Date) - $t0).TotalMilliseconds + "ms -> banner v" + $ver)
      }
    }
  } catch { }
  $h = (Get-FileHash -LiteralPath $target -Algorithm SHA256 -ErrorAction SilentlyContinue).Hash
  if ($h) { $finalSha = $h.ToLower() }
  if ($finalSha -eq $ExpectedSha256.ToLower()) { break }
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
    if ($p) { $owned += $p.ProcessName }
  }
}
$owned = @($owned | Sort-Object -Unique)
Write-Output ("  " + ($(if ($owned.Count) { $owned -join ', ' } else { '(none)' })))

Write-Output ""
Write-Output "=== verdict ==="
if ($finalSha -eq $ExpectedSha256.ToLower()) {
  Write-Output ("  [PASS] the app updated itself to the published build (banner v" + $finalVer + ") with no user action")
} else {
  Write-Output ("  [FAIL] exe is still " + $finalSha + "; the update did not land (banner v" + $finalVer + ")")
  Write-Output ("         expected " + $ExpectedSha256.ToLower())
}
$console = @($owned | Where-Object { $consoleNames -contains $_.ToLower() })
if ($console.Count -eq 0) {
  Write-Output "  [PASS] the upgrade started no console-hosting child of the app"
} else {
  Write-Output ("  [FAIL] console children during upgrade: " + ($console -join ', '))
}

# The updater's own narration: the log is the only witness to the phases, and a
# pass on the hash alone would not show whether it was silent along the way.
$log = Join-Path $env:USERPROFILE '.aimyria\logs\aimyria-one.log'
Write-Output ""
Write-Output "=== updater log lines from this run ==="
Get-Content -LiteralPath $log -Tail 80 -ErrorAction SilentlyContinue |
  Where-Object { $_ -match '\[updater\]' } | Select-Object -Last 12 | ForEach-Object { Write-Output ("  " + $_) }
