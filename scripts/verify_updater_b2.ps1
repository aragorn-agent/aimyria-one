# B2 -- the silent self-update, run for real, both paths.
#
# WHY THIS IS A SCRIPT AND NOT A LIST OF COMMANDS
#
# "It updated itself" is a claim about a sequence of filesystem states over time,
# and the states are transient: `.old` exists only between the rename and the
# restarted process's cleanup, and the log lines that prove the sequence scroll by.
# A hand-run test reports whichever of those the operator happened to look at. This
# one records the whole trajectory.
#
# THE SAFETY RULES THIS RUNS UNDER (see the caller's notes)
#
#   * no deep link is passed to the test instance, so no confirmation dialog opens
#     and nothing is ever written to a real client config;
#   * nothing is brought to the foreground, no window is activated, no key is sent.
#     The machine is in use by a person. The test observes from outside;
#   * THE REAL INSTALL IS NEVER RUN. The test exe is a copy under `%TEMP%`, and the
#     app config dir is redirected with `CC_SWITCH_TEST_HOME` so its database and
#     logs live in the test tree;
#   * the two protocol keys and the Run value ARE repointed as a side effect --
#     `first_run::maybe_run` re-registers on every launch, pointing at
#     `current_exe()` -- so the script snapshots them first and restores them at the
#     end, verifying the restored target exists on disk. Skipping that step is how
#     a test run leaves a user's 「导入到CCS」 pointing at a deleted temp file.
#
# TWO PATHS
#
#   success  version.json describes a newer version by its true sha256 -> the old
#            exe is replaced, the payload's hash is what lands on disk, `.old`
#            holds the previous bytes, the process restarts itself, and the new
#            process logs the new version.
#   failure  version.json has a deliberately wrong sha256 -> nothing is replaced,
#            the exe is byte-identical afterwards, no leftovers, the pid never
#            changes, and the log says exactly why.

<#
.SYNOPSIS  B2: exercise the silent self-updater for real, success and failure.

.DESCRIPTION
  "It updated itself" is a claim about a sequence of filesystem states over time,
  and the states are transient: `.old` exists only between the rename and the
  restarted process's cleanup, and the log lines that prove the sequence scroll by.
  A hand-run test reports whichever of those the operator happened to look at. This
  one records the whole trajectory and prints the raw log lines that prove it.

  SAFETY RULES THIS RUNS UNDER
    * no deep link is passed to the test instance, so no confirmation dialog opens
      and nothing is ever written to a real client config;
    * nothing is brought to the foreground, no window is activated, no key is sent.
      The machine is in use by a person; this test only observes from outside;
    * THE REAL INSTALL IS NEVER RUN. The test exe is a copy under %TEMP% and the
      app config dir is redirected with CC_SWITCH_TEST_HOME, so its database and
      logs live in the test tree;
    * THE TWO PROTOCOL KEYS AND THE RUN VALUE ARE REPOINTED as a side effect --
      `first_run::maybe_run` re-registers on every launch, pointing at
      `current_exe()` -- so this script snapshots them first and restores them at
      the end, verifying the restored target exists on disk. Skipping that step is
      how a test run leaves a user's 「导入到CCS」 pointing at a deleted temp file.

.PARAMETER OldExe
  The copy that runs first (the "installed" version).
.PARAMETER NewExe
  The copy that will be published and should replace it (the "new" version).

.EXAMPLE
  pwsh -File scripts\verify_updater_b2.ps1 `
    -OldExe E:\target-cache\aimyria-v2\release\aimyria-one.exe `
    -NewExe E:\cc-switch-main\cc-switch-main\release\AimyriaOne-x64.exe
#>
param(
  [Parameter(Mandatory = $true)][string]$OldExe,
  [Parameter(Mandatory = $true)][string]$NewExe,
  [string]$Work = "",
  [int]$PortSuccess = 8799,
  [int]$PortFailure = 8800,
  [int]$TimeoutSeconds = 120,
  # The version the served manifest advertises. MUST be strictly greater than the
  # installed build's own version, or the updater correctly concludes "up to date" and
  # never exercises the download/replace path. Passing 1.0.1 here would test nothing --
  # which is exactly what happened on the first attempt: the log said
  # "up to date: running v1.0.1, manifest has v1.0.1" and every later assertion was moot.
  [string]$ManifestVersion = "1.0.2",
  [string]$NodeExe = "node"
)

$ErrorActionPreference = "Continue"

if (-not $Work) { $Work = Join-Path $env:TEMP "aimyria-updater-b2" }
$results = [System.Collections.Generic.List[string]]::new()

function Section($t) { Write-Output ""; Write-Output ("=== " + $t + " ===") }
function Pass($m) { Write-Output ("  [PASS] " + $m); $results.Add("PASS  $m") }
function Fail($m) { Write-Output ("  [FAIL] " + $m); $results.Add("FAIL  $m") }
function Info($m) { Write-Output ("  " + $m) }

# ---------------------------------------------------------------------------
# registry snapshot / restore
# ---------------------------------------------------------------------------
$RegAimyria = "HKCU\Software\Classes\aimyria\shell\open\command"
$RegCcswitch = "HKCU\Software\Classes\ccswitch\shell\open\command"
$RegRun = "HKCU\Software\Microsoft\Windows\CurrentVersion\Run"

function Read-RegDefault($key) {
  $q = reg query $key /ve 2>&1
  return (($q | Select-String 'REG_SZ' | ForEach-Object { ($_ -split 'REG_SZ', 2)[1].Trim() }) -join "")
}
function Read-RunValue($name) {
  $q = reg query $RegRun /v $name 2>&1
  return (($q | Select-String 'REG_SZ' | ForEach-Object { ($_ -split 'REG_SZ', 2)[1].Trim() }) -join "")
}

Section "0. registry snapshot before anything runs"
$beforeAimyria = Read-RegDefault $RegAimyria
$beforeCcswitch = Read-RegDefault $RegCcswitch
$beforeRun = Read-RunValue "AimyriA go"
Info ("aimyria  open command : " + $beforeAimyria)
Info ("ccswitch open command : " + $beforeCcswitch)
Info ("Run 'AimyriA go'       : " + $beforeRun)

# ---------------------------------------------------------------------------
# processes
# ---------------------------------------------------------------------------
# Matched on executable path, not process name: the process name comes from the
# file name, and a copy delivered as AimyriaOne-x64.exe is named AimyriaOne-x64.
function Get-TestProcesses {
  @(Get-Process -ErrorAction SilentlyContinue |
      Where-Object { $_.Path -and $_.Path.StartsWith($Work, [StringComparison]::OrdinalIgnoreCase) })
}
function Get-InstallProcesses {
  @(Get-Process -ErrorAction SilentlyContinue |
      Where-Object { $_.Path -and ($_.Path -match 'aimyria' -or $_.Path -match 'Aimyria') -and
                     -not $_.Path.StartsWith($Work, [StringComparison]::OrdinalIgnoreCase) })
}

Section "1. preconditions"
$install = Get-InstallProcesses
if ($install.Count -gt 0) {
  # A running instance would make the test launches no-ops: the single-instance
  # plugin forwards the args and the new process exits 0 immediately, which is
  # indistinguishable from a crash if you only read the exit code.
  Fail ("an instance of the real install is running: " + (($install | ForEach-Object { "$($_.Id) $($_.Path)" }) -join "; "))
  Info "stop it before running this script; a second launch would be absorbed by single-instance"
} else {
  Pass "no aimyria instance running outside the test tree"
}

Get-TestProcesses | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

foreach ($p in @($OldExe, $NewExe)) {
  if (-not (Test-Path -LiteralPath $p)) { Write-Output "missing input exe: $p"; exit 2 }
}
$oldHash = (Get-FileHash -LiteralPath $OldExe -Algorithm SHA256).Hash.ToLower()
$newHash = (Get-FileHash -LiteralPath $NewExe -Algorithm SHA256).Hash.ToLower()
Info ("old exe : $OldExe")
Info ("  sha256: $oldHash  ($((Get-Item $OldExe).Length) bytes)")
Info ("new exe : $NewExe")
Info ("  sha256: $newHash  ($((Get-Item $NewExe).Length) bytes)")
if ($oldHash -eq $newHash) { Write-Output "the two exes are identical; there is nothing to update"; exit 2 }

# ---------------------------------------------------------------------------
# layout
# ---------------------------------------------------------------------------
if (Test-Path -LiteralPath $Work) { Remove-Item -LiteralPath $Work -Recurse -Force }
foreach ($d in @("success", "failure", "payload-ok", "payload-bad", "home-success", "home-failure")) {
  New-Item -ItemType Directory -Force -Path (Join-Path $Work $d) | Out-Null
}

# The served tree carries the exact release layout the client asks for:
# /releases/latest/download/version.json next to the payload it describes.
$serveOk = Join-Path $Work "payload-ok\releases\latest\download"
$serveBad = Join-Path $Work "payload-bad\releases\latest\download"
New-Item -ItemType Directory -Force -Path $serveOk, $serveBad | Out-Null

Copy-Item -LiteralPath $NewExe (Join-Path $serveOk "AimyriaOne-x64.exe") -Force
"$newHash  AimyriaOne-x64.exe" | Set-Content (Join-Path $serveOk "AimyriaOne-x64.exe.sha256") -Encoding ASCII

$versionJson = [ordered]@{
  version = $ManifestVersion
  url     = "http://127.0.0.1:$PortSuccess/releases/latest/download/AimyriaOne-x64.exe"
  sha256  = $newHash
  notes   = "silent self-update acceptance run (success path)"
  pubDate = (Get-Date).ToString("yyyy-MM-ddTHH:mm:sszzz")
}
$versionJson | ConvertTo-Json | Set-Content (Join-Path $serveOk "version.json") -Encoding ASCII
# Verify the manifests are BOM-free. A UTF-8 BOM is the difference between a
# working channel and an opaque "error decoding response body": serde_json refuses
# a BOM-prefixed document. PowerShell 5.1's `Set-Content -Encoding UTF8` writes one,
# so this is a real trap for whoever edits this script next -- assert the bytes.
foreach ($mf in @((Join-Path $serveOk "version.json"), (Join-Path $serveBad "version.json"))) {
  $raw = [System.IO.File]::ReadAllBytes($mf)
  $hasBom = ($raw.Length -ge 3 -and $raw[0] -eq 0xEF -and $raw[1] -eq 0xBB -and $raw[2] -eq 0xBF)
  if ($hasBom) { Fail ("manifest has a UTF-8 BOM: " + $mf + " -- the updater will refuse it (that is the bug this run is meant to exercise, not reproduce)") }
  else { Pass ("manifest is BOM-free: " + (Split-Path $mf -Parent | Split-Path -Leaf) + " (" + $raw.Length + " bytes)") }
}
Info ("served (success): " + (Join-Path $serveOk "version.json"))

# The failure path is the same manifest with the hash of *different* bytes: the
# payload is real and intact, the manifest lies about it. That is the shape of a
# truncated upload or a copied-from-the-wrong-file hash, which is the failure this
# gate exists for.
Copy-Item -LiteralPath $NewExe (Join-Path $serveBad "AimyriaOne-x64.exe") -Force
$wrongHash = ([string]::Join("", (1..64 | ForEach-Object { "0" })))
$versionJsonBad = [ordered]@{
  version = $ManifestVersion
  url     = "http://127.0.0.1:$PortFailure/releases/latest/download/AimyriaOne-x64.exe"
  sha256  = $wrongHash
  notes   = "silent self-update acceptance run (failure path: wrong sha256 on purpose)"
  pubDate = (Get-Date).ToString("yyyy-MM-ddTHH:mm:sszzz")
}
$versionJsonBad | ConvertTo-Json | Set-Content (Join-Path $serveBad "version.json") -Encoding ASCII
Info ("served (failure): " + (Join-Path $serveBad "version.json"))
Info ("  manifest claims sha256 = $wrongHash")
Info ("  payload really is      = $newHash")

# The test instance is named like the delivered file. The resolved exe path is what
# `first_run` re-registers, and it has to be one the app can read back.
$testExeName = "AimyriaOne-x64.exe"
$successExe = Join-Path $Work "success\$testExeName"
$failureExe = Join-Path $Work "failure\$testExeName"
Copy-Item -LiteralPath $OldExe $successExe -Force
Copy-Item -LiteralPath $OldExe $failureExe -Force

# ---------------------------------------------------------------------------
# servers
# ---------------------------------------------------------------------------
Section "2. local release servers"
# Started as background JOBS, not Start-Process -RedirectStandardOutput.
#
# On this machine the redirect path throws
#   "Item has already been added. Key in dictionary: 'NO_PROXY' / 'no_proxy'"
# Windows environment variables are case-insensitive, so a machine carrying both
# spellings has a duplicate key, and the redirect builds a case-sensitive dictionary.
# The error is non-terminating, $p comes back $null, nothing listens, and every later
# assertion then "passes" for the wrong reason: an updater cannot fetch from a server
# that was never started. The previous run of this script failed exactly that way.
$serverJobs = @()
$serverLogs = @()
foreach ($spec in @(
    @{ dir = (Join-Path $Work "payload-ok");  port = $PortSuccess; name = "ok" },
    @{ dir = (Join-Path $Work "payload-bad"); port = $PortFailure; name = "bad" }
  )) {
  $serverScript = Join-Path (Split-Path -Parent $PSScriptRoot) "scripts\serve_updater_release.mjs"
  $job = Start-Job -ScriptBlock {
    param($svc, $dir, $port, $node)
    & $node $svc $dir $port 2>&1
  } -ArgumentList $serverScript, $spec.dir, $spec.port, $NodeExe
  $serverJobs += $job
  $serverLogs += $spec.name
  Info ("started $($spec.name) server as job $($job.Id) on port $($spec.port) serving $($spec.dir)")
}

# Poll the port instead of sleeping a fixed time: a server that failed to bind has to
# be reported as such, not raced past into a meaningless success.
$serverUp = @{}
foreach ($spec in @(
    @{ port = $PortSuccess; name = "ok" },
    @{ port = $PortFailure; name = "bad" }
  )) {
  $url = "http://127.0.0.1:$($spec.port)/releases/latest/download/version.json"
  $serverUp[$spec.name] = $false
  for ($attempt = 0; $attempt -lt 60; $attempt++) {
    try {
      $r = Invoke-WebRequest -Uri $url -TimeoutSec 3 -UseBasicParsing -ErrorAction Stop
      Info ("GET $url -> " + [int]$r.StatusCode + "  content-type=" + $r.Headers['Content-Type'] + "  cache-control=" + $r.Headers['Cache-Control'])
      $serverUp[$spec.name] = $true
      if ($r.Headers['Cache-Control'] -match 'no-store') { Pass "manifest for the $($spec.name) server is served no-store" }
      else { Fail "manifest for the $($spec.name) server is cacheable (" + $r.Headers['Cache-Control'] + ")" }
      break
    } catch {
      Start-Sleep -Milliseconds 250
    }
  }
  if (-not $serverUp[$spec.name]) {
    Fail ("GET $url never became reachable after 15s -- no assertion below can be trusted")
    foreach ($job in $serverJobs) {
      Info ("  job $($job.Id) state=" + $job.State)
      Receive-Job -Job $job -Keep -ErrorAction SilentlyContinue | Select-Object -First 6 | ForEach-Object { Info ("    " + $_) }
    }
  }
}

# ---------------------------------------------------------------------------
# launcher
# ---------------------------------------------------------------------------
# Environment, not arguments. `CC_SWITCH_TEST_HOME` is the app's own isolation
# hook: it short-circuits the Windows HOME fallback in `config.rs`, so the test
# instance's database, crash log and log directory all land in the test tree and
# the real `%USERPROFILE%\.aimyria` is untouched. The three AIMYRIA_UPDATE_* vars
# are the updater's test hooks: the manifest URL moves, the clock shortens, and
# loopback http becomes allowable. Nothing else about the updater changes.
function Start-TestInstance {
  param([string]$Exe, [string]$HomeDir, [string]$ManifestUrl)
  # Start from a clean app dir. Without this the log would carry a previous run's
  # lines, and `v1.0.1 started` could be read off an earlier attempt rather than
  # off the restart this run is supposed to prove.
  Remove-Item -LiteralPath $HomeDir -Recurse -Force -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force -Path $HomeDir | Out-Null
  $env:CC_SWITCH_TEST_HOME = $HomeDir
  $env:AIMYRIA_UPDATE_TEST_CLOCK = "1"
  $env:AIMYRIA_UPDATE_MANIFEST_URL = $ManifestUrl
  Remove-Item Env:\AIMYRIA_UPDATE_DRYRUN -ErrorAction SilentlyContinue
  $env:HOME = $HomeDir
  # -PassThru, no -WindowStyle: the app creates its window hidden by design, and
  # this script never shows, focuses or activates anything.
  $proc = Start-Process -FilePath $Exe -WorkingDirectory (Split-Path -Parent $Exe) -PassThru
  Remove-Item Env:\AIMYRIA_UPDATE_TEST_CLOCK, Env:\AIMYRIA_UPDATE_MANIFEST_URL, Env:\CC_SWITCH_TEST_HOME, Env:\HOME -ErrorAction SilentlyContinue
  return $proc
}

function Get-LogPath($homeDir) { Join-Path $homeDir ".aimyria\logs\aimyria-one.log" }
function Get-LogText($homeDir) {
  $p = Get-LogPath $homeDir
  if (Test-Path -LiteralPath $p) { return (Get-Content -LiteralPath $p -Raw -ErrorAction SilentlyContinue) }
  return ""
}

# ---------------------------------------------------------------------------
# success path
# ---------------------------------------------------------------------------
Section "3. SUCCESS PATH -- wrong nothing, real payload, real sha256"
$successHome = Join-Path $Work "home-success"
$manifestOk = "http://127.0.0.1:$PortSuccess/releases/latest/download/version.json"
Info ("launching $successExe")
Info ("  manifest  = $manifestOk")
Info ("  home      = $successHome")
$t0 = Get-Date
$proc = Start-TestInstance -Exe $successExe -HomeDir $successHome -ManifestUrl $manifestOk
$startPid = $proc.Id
Info ("pid = $startPid at " + $t0.ToString("HH:mm:ss"))

# Poll the trajectory rather than sampling once. `.old` is short-lived by design --
# the restarted process deletes it in startup_cleanup -- so "was it ever there" is
# the only honest question, and only polling can answer it.
$sawOld = $false
$oldHashSeen = ""
$sawNewLeftover = $false
$restartPid = $null
$deadline = $t0.AddSeconds($TimeoutSeconds)
$exeNow = $oldHash
while ((Get-Date) -lt $deadline) {
  $newPath = "$successExe.new"
  $oldPath = "$successExe.old"
  if (Test-Path -LiteralPath $oldPath) {
    $sawOld = $true
    if (-not $oldHashSeen) { $oldHashSeen = (Get-FileHash -LiteralPath $oldPath -Algorithm SHA256).Hash.ToLower() }
  }
  if (Test-Path -LiteralPath $newPath) { $sawNewLeftover = $true }
  if (Test-Path -LiteralPath $successExe) { $exeNow = (Get-FileHash -LiteralPath $successExe -Algorithm SHA256).Hash.ToLower() }
  # A restarted instance is a different pid running from the same path.
  $running = Get-TestProcesses | Where-Object { $_.Path -eq $successExe }
  $pids = @($running | Select-Object -ExpandProperty Id)
  if ($pids -and ($pids -notcontains $startPid)) { $restartPid = ($pids | Select-Object -First 1) }
  if ($exeNow -eq $newHash -and $restartPid) { break }
  Start-Sleep -Milliseconds 300
}
$elapsed = [Math]::Round(((Get-Date) - $t0).TotalSeconds, 1)
Info ("settled after ${elapsed}s")

$logText = Get-LogText $successHome
$logPath = Get-LogPath $successHome
Info ("log: $logPath  ($($logText.Length) chars)")

Write-Output "  -- the sequence, from the log --"
$markers = @(
  "checking manifest",
  "manifest: version=",
  "update available",
  "downloading",
  "downloaded",
  "sha256 verified",
  "staged",
  "renamed",
  "replaced",
  "retained for one cycle",
  "installed at",
  "restarting into",
  "auto-update armed",
  "previous update completed",
  "running v"
)
foreach ($m in $markers) {
  $hit = $logText -split "`r?`n" | Where-Object { $_ -like "*[updater]*" -and $_ -like "*$m*" }
  if ($hit) { $hit | Select-Object -First 2 | ForEach-Object { Info ("    " + $_.Trim()) } }
  else { Info ("    (no line containing '$m')") }
}

Info "  -- assertion: the payload is what is on disk now --"
Info ("    exe sha256 = $exeNow")
Info ("    payload    = $newHash")
if ($exeNow -eq $newHash) { Pass "B2 success: the exe on disk is byte-identical to the published payload" }
else { Fail "B2 success: the exe was not replaced (on disk $exeNow)" }

Info "  -- assertion: the previous exe was retained for one cycle --"
# This assertion is only meaningful if an update actually happened. An earlier
# version PASSed here whenever `.old` was absent, on the theory that the restarted
# process had already cleaned it up -- so it reported PASS in a run where the server
# had never started and nothing was ever replaced. "No update" and "update that
# cleaned up after itself" must not share a verdict.
$replaced = ($exeNow -eq $newHash)
if (-not $replaced) {
  Info ("    (not applicable: nothing was replaced, so there is no rollback copy to speak of)")
  Fail "B2 success: '.old' retention could not be assessed because the replacement never happened"
} elseif ($sawOld) {
  Info ("    .old observed, sha256 = $oldHashSeen")
  if ($oldHashSeen -eq $oldHash) { Pass "B2 success: '.old' held the previous exe, byte-identical (rollback copy existed)" }
  else { Fail "B2 success: '.old' did not hold the previous exe" }
} else {
  Pass "B2 success: the replacement happened and '.old' was already cleaned up by the restarted process"
  Info "    (not observed in a poll, which is expected: the new process removes it on its first launch)"
}

Info "  -- assertion: the restarted process logs the new version --"
$restartEvidence = $logText -split "`r?`n" | Where-Object { $_ -match '=== AimyriA go v1\.0\.2 started ===' }
if ($restartEvidence) {
  Pass "B2 success: the restarted instance logged 'v1.0.2 started'"
  $restartEvidence | Select-Object -First 1 | ForEach-Object { Info ("    " + $_.Trim()) }
} else {
  $anyStarted = $logText -split "`r?`n" | Where-Object { $_ -match 'started ===' }
  Fail "B2 success: no 'v1.0.2 started' line; saw: " + (($anyStarted | Select-Object -First 3) -join " | ")
}
$cleanupLines = $logText -split "`r?`n" | Where-Object { $_ -match '\[updater\].*(previous update completed|running v)' }
if ($cleanupLines) {
  Pass "B2 success: the new process reported its own version from startup_cleanup"
  $cleanupLines | Select-Object -First 2 | ForEach-Object { Info ("    " + $_.Trim()) }
} else { Fail "B2 success: startup_cleanup left no version line" }

if ($restartPid) { Pass "B2 success: the app restarted itself (new pid $restartPid, was $startPid)" }
else { Fail "B2 success: no restart observed -- the replaced exe never ran" }
if ($sawNewLeftover) { Info "    (a .new was seen transiently during staging, as intended)" }

Info "  -- stopping the restarted instance --"
Get-TestProcesses | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

# ---------------------------------------------------------------------------
# failure path
# ---------------------------------------------------------------------------
Section "4. FAILURE PATH -- manifest sha256 is wrong on purpose"
$failureHome = Join-Path $Work "home-failure"
$manifestBad = "http://127.0.0.1:$PortFailure/releases/latest/download/version.json"
Info ("launching $failureExe")
Info ("  manifest  = $manifestBad")
Info ("  home      = $failureHome")
$t1 = Get-Date
$procFail = Start-TestInstance -Exe $failureExe -HomeDir $failureHome -ManifestUrl $manifestBad
$failPid = $procFail.Id
Info ("pid = $failPid at " + $t1.ToString("HH:mm:ss"))

# Long enough for two scheduled checks at the shortened period, so the run also
# shows the cadence rather than a single attempt.
Start-Sleep -Seconds 24

$failExeHash = (Get-FileHash -LiteralPath $failureExe -Algorithm SHA256).Hash.ToLower()
$failAlive = [bool](Get-Process -Id $failPid -ErrorAction SilentlyContinue)
$failLog = Get-LogText $failureHome
$failLogPath = Get-LogPath $failureHome
Info ("log: $failLogPath  ($($failLog.Length) chars)")
Info ("process $failPid still alive: $failAlive")

Info "  -- the refusal, from the log --"
$refusals = $failLog -split "`r?`n" | Where-Object { $_ -match '\[updater\]' -and $_ -match 'sha256 mismatch|discarding|NOT replacing|no replacement this round' }
if ($refusals) { $refusals | ForEach-Object { Info ("    " + $_.Trim()) } }
else { Info "    (no refusal lines found)" }

Info "  -- assertion: nothing was replaced --"
Info ("    exe sha256 now      = $failExeHash")
Info ("    exe sha256 before   = $oldHash")
if ($failExeHash -eq $oldHash) { Pass "B2 failure: the exe is byte-identical to what it was before the attempt" }
else { Fail "B2 failure: the exe changed despite the bad checksum" }

Info "  -- assertion: no leftovers --"
$leftovers = @()
foreach ($suffix in @(".new", ".old")) {
  if (Test-Path -LiteralPath "$failureExe$suffix") { $leftovers += "$failureExe$suffix" }
}
if ($leftovers.Count -eq 0) { Pass "B2 failure: no .new and no .old left beside the exe" }
else { Fail ("B2 failure: leftovers remain -- " + ($leftovers -join ", ")) }

if ($failAlive) { Pass "B2 failure: the instance never restarted (same pid $failPid still running) -- no update was installed" }
else { Fail "B2 failure: the process exited; it should still be running on the old version" }

if ($refusals -and ($refusals -join " ") -match 'sha256 mismatch') {
  Pass "B2 failure: the log states the reason (manifest sha256 mismatch)"
} else { Fail "B2 failure: the log does not name the sha256 mismatch" }

# One failed round must not turn into a retry loop. The shortened clock is 15 s;
# anything materially faster than that is a storm.
$checkLines = @($failLog -split "`r?`n" | Where-Object { $_ -match '\[updater\] checking manifest' })
Info ("  scheduled checks observed in " + [Math]::Round(((Get-Date) - $t1).TotalSeconds,1) + "s: " + $checkLines.Count)
if ($checkLines.Count -le 3) { Pass "B2 failure: one attempt per scheduled window, no retry storm" }
else { Fail ("B2 failure: " + $checkLines.Count + " attempts -- retrying too fast") }

Info "  -- stopping the failure-path instance --"
Get-TestProcesses | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

# ---------------------------------------------------------------------------
# servers down, registry restored
# ---------------------------------------------------------------------------
Section "5. teardown"
foreach ($job in $serverJobs) {
  Info ("releasing server job $($job.Id):")
  Receive-Job -Job $job -ErrorAction SilentlyContinue | Select-Object -Last 12 | ForEach-Object { Info ("    " + $_) }
  Stop-Job -Job $job -ErrorAction SilentlyContinue
  Remove-Job -Job $job -Force -ErrorAction SilentlyContinue
}
Info "release servers stopped"

$remaining = Get-TestProcesses
if ($remaining.Count -eq 0) { Pass "no test-tree instance left running" }
else {
  Info ("killing " + $remaining.Count + " remaining")
  $remaining | Stop-Process -Force -ErrorAction SilentlyContinue
}

# --- the restoration that matters -------------------------------------------
# Both protocol keys and (potentially) the Run value were repointed at test-tree
# paths by the test instances' own launch bootstrap. Leaving them there would send
# every future 「导入到CCS」 on this machine to a deleted file.
# Stray processes would keep the tree locked and could re-register after the restore
# below, so they go first -- and the restore is re-checked afterwards.
Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($Work, [StringComparison]::OrdinalIgnoreCase) } | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

Section "6. restore the registry (the test instances repointed it)"
$restoreAimyria = Read-RegDefault $RegAimyria
$restoreCcswitch = Read-RegDefault $RegCcswitch
$restoreRun = Read-RunValue "AimyriA go"
Info ("after the run -- aimyria  : " + $restoreAimyria)
Info ("after the run -- ccswitch : " + $restoreCcswitch)
Info ("after the run -- Run      : " + $restoreRun)

function Get-ExeFromCommand($command) {
  if ($command -match '^\s*"([^"]+)"') { return $matches[1] }
  if ($command -match '^\s*([^\s]+)') { return $matches[1] }
  return ""
}

# The restore target must be a REAL INSTALL, never a test copy.
#
# This script snapshots whatever is registered when it starts, and restores that at
# the end. That is right when the run starts from a clean machine -- and wrong when
# the previous run left a temp copy registered, because then this run faithfully
# "restores" a path that is about to be deleted. That is exactly what happened: a
# B2 run started while %TEMP%\aimyria-second-launch\run\... was still registered, and
# dutifully put it back at the end, leaving the machine pointing at a doomed copy.
#
# So a target under TEMP (or under this run's work tree) is treated as
# "no trustworthy snapshot": fall back to the real per-user install, which is what
# the machine should point at anyway.
$target = ""
if ($beforeAimyria) { $target = Get-ExeFromCommand $beforeAimyria }
$realInstall = Join-Path $env:LOCALAPPDATA "Aimyria\AimyriaOne.exe"
$targetIsTestCopy = $target -and (($target -match '[\\/]Temp[\\/]') -or $target.StartsWith($Work, [StringComparison]::OrdinalIgnoreCase))
if ($targetIsTestCopy) {
  Info ("IGNORING the pre-run target because it is a test copy: " + $target)
  if (Test-Path -LiteralPath $realInstall) { $target = $realInstall }
  Info ("falling back to the real install: " + $target)
}
Info ("restoring the pre-run target: $target")
if ($target -and (Test-Path -LiteralPath $target)) {
  # Written through the .NET registry API, NOT `reg add`.
  #
  # `reg add ... /d '"C:\path\app.exe" "%1"'` was measured dropping the leading
  # quote, storing `C:\path\app.exe "%1"`. That happens to still launch for a path
  # without spaces and silently breaks the moment the install path has one -- and it
  # is not byte-identical to the value being restored, which is the whole point of a
  # restore. SetValue with an explicit String kind stores exactly the characters given.
  $q = [char]34
  $cmd = $q + $target + $q + ' ' + $q + '%1' + $q
  foreach ($pair in @(
      @{ sub = 'Software\Classes\aimyria\shell\open\command'; name = '' },
      @{ sub = 'Software\Classes\ccswitch\shell\open\command'; name = '' }
    )) {
    $k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($pair.sub, $true)
    if ($k) { try { $k.SetValue($pair.name, $cmd, [Microsoft.Win32.RegistryValueKind]::String) } finally { $k.Close() } }
  }
  if ($beforeRun) {
    $kr = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Run', $true)
    if ($kr) { try { $kr.SetValue('AimyriA go', $beforeRun, [Microsoft.Win32.RegistryValueKind]::String) } finally { $kr.Close() } }
  }
  Start-Sleep -Seconds 1
  $nowAimyria = Read-RegDefault $RegAimyria
  $nowCcswitch = Read-RegDefault $RegCcswitch
  $nowRun = Read-RunValue "AimyriA go"
  Info ("restored -- aimyria  : " + $nowAimyria)
  Info ("restored -- ccswitch : " + $nowCcswitch)
  Info ("restored -- Run      : " + $nowRun)
  $ok = $true
  if ($nowAimyria -notmatch [regex]::Escape($target)) { $ok = $false; Fail "aimyria:// was not restored" }
  if ($nowCcswitch -notmatch [regex]::Escape($target)) { $ok = $false; Fail "ccswitch:// was not restored" }
  if ($beforeRun -and $nowRun -ne $beforeRun) { $ok = $false; Fail "the Run value was not restored" }
  if ($ok) { Pass "registry restored: aimyria://, ccswitch:// and the Run value all point back at the pre-run target" }
} else {
  Fail "the pre-run aimyria:// target could not be parsed or no longer exists: '$target'"
  Info "MANUAL ACTION REQUIRED -- set both protocol keys to the real install path"
}

Section "summary"
$results | ForEach-Object { Write-Output ("  " + $_) }
$failed = @($results | Where-Object { $_ -like "FAIL*" }).Count
Write-Output ("  failures: " + $failed)
Write-Output ("  test tree (kept for inspection): " + $Work)
exit $(if ($failed -eq 0) { 0 } else { 1 })
