# Runtime proof that the updater tolerates a BOM-prefixed manifest.
#
# WHY THIS EXISTS, SEPARATELY FROM THE UNIT TEST
#
# `updater::tests::a_utf8_bomprefix_does_not_break_the_manifest` proves that
# `strip_utf8_bom` + `serde_json::from_slice` handle a BOM. It does NOT prove that
# the *shipping code path* uses them: the original defect lived in the difference
# between `serde_json::from_str` (what the unit test calls) and `reqwest::Response`
# `.json()` (what the app calls). A unit test that passes while the wired-up code
# still calls `.json()` is precisely the failure this project has now hit four
# times -- "looks fixed, was never exercised".
#
# So this script starts a real HTTP server, serves a manifest that really begins
# with EF BB BF, launches a real copy of the app pointed at it, and reads the app's
# own log. The assertion is about what the *binary* did.
#
# The pair matters as much as the assertion:
#   case BOM      the manifest has EF BB BF -> the app must PARSE it
#   case NO-BOM   the same manifest without it -> the app must PARSE it too
# A run that passes the BOM case but not the plain case would mean the parser is
# broken in some other way, and both cases are reported.
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [int]$PortBom = 8801,
  [int]$PortPlain = 8802,
  [string]$Work = "",
  [int]$WaitSeconds = 25
)

$ErrorActionPreference = "Continue"
$results = [System.Collections.Generic.List[string]]::new()
function Pass($m) { Write-Output ("  [PASS] " + $m); $results.Add("PASS  " + $m) }
function Fail($m) { Write-Output ("  [FAIL] " + $m); $results.Add("FAIL  $m") }
function Info($m) { Write-Output ("  " + $m) }

if (-not $Work) { $Work = Join-Path $env:TEMP "aimyria-bom-proof" }

# ---------------------------------------------------------------------------
# Which version does this exe report? The manifest must advertise something the
# updater will act on, and "act on" means strictly newer -- a manifest naming the
# same version produces "up to date" and proves only that comparison ran.
# ---------------------------------------------------------------------------
$bytes = [System.IO.File]::ReadAllBytes($Exe)
$ascii = [System.Text.Encoding]::ASCII.GetString($bytes)
$current = $null
foreach ($v in @("1.0.9", "1.0.8", "1.0.7", "1.0.6", "1.0.5", "1.0.4", "1.0.3", "1.0.2", "1.0.1", "1.0.0")) {
  if ($ascii.Contains("AimyriA go v$v started")) { $current = $v; break }
}
if (-not $current) { Write-Output "could not determine the exe's version from its strings"; exit 2 }
$newer = ([version]$current).Major.ToString() + "." + ([version]$current).Minor.ToString() + "." + (([version]$current).Build + 1).ToString()
Write-Output ("exe      : " + $Exe)
Write-Output ("version  : v" + $current + "  -> the manifest will advertise v" + $newer)
Write-Output ""

# ---------------------------------------------------------------------------
# Build the two served trees
# ---------------------------------------------------------------------------
if (Test-Path -LiteralPath $Work) { Remove-Item -LiteralPath $Work -Recurse -Force }
$trees = @{}
foreach ($case in @(
    @{ name = "bom"; port = $PortBom },
    @{ name = "plain"; port = $PortPlain }
  )) {
  $root = Join-Path $Work ($case.name + "\releases\latest\download")
  New-Item -ItemType Directory -Force -Path $root | Out-Null
  # The payload is a copy of this same exe: this run is about PARSING, so the
  # download and the checksum must be real but the bytes need not differ.
  Copy-Item -LiteralPath $Exe (Join-Path $root "AimyriaOne-x64.exe") -Force
  $sha = (Get-FileHash -LiteralPath $Exe -Algorithm SHA256).Hash.ToLower()
  $json = [ordered]@{
    version = $newer
    url     = "http://127.0.0.1:$($case.port)/releases/latest/download/AimyriaOne-x64.exe"
    sha256  = $sha
    notes   = "BOM tolerance proof (" + $case.name + ")"
    pubDate = (Get-Date).ToString("yyyy-MM-ddTHH:mm:sszzz")
  } | ConvertTo-Json -Compress

  $path = Join-Path $root "version.json"
  if ($case.name -eq "bom") {
    # Written byte by byte so the BOM is certain -- `Set-Content -Encoding UTF8` on
    # PowerShell 5.1 would add one, but relying on that would make the test depend
    # on which PowerShell runs it, and 7.x does not add one. The point is the bytes.
    $bom = [byte[]](0xEF, 0xBB, 0xBF)
    $body = [System.Text.Encoding]::UTF8.GetBytes($json)
    [System.IO.File]::WriteAllBytes($path, [byte[]]($bom + $body))
  } else {
    [System.IO.File]::WriteAllBytes($path, [System.Text.Encoding]::UTF8.GetBytes($json))
  }

  $written = [System.IO.File]::ReadAllBytes($path)
  $hasBom = ($written.Length -ge 3 -and $written[0] -eq 0xEF -and $written[1] -eq 0xBB -and $written[2] -eq 0xBF)
  $head = ($written[0..5] | ForEach-Object { '{0:X2}' -f $_ }) -join ' '
  Info ("case " + $case.name + ": wrote " + $written.Length + " bytes, first 6 = " + $head + ", hasBom=" + $hasBom)
  if ($case.name -eq "bom" -and -not $hasBom) { Write-Output "the BOM case is not actually BOM-prefixed; aborting"; exit 2 }
  if ($case.name -eq "plain" -and $hasBom) { Write-Output "the plain case accidentally has a BOM; aborting"; exit 2 }

  $trees[$case.name] = @{ root = (Join-Path $Work $case.name); port = $case.port; manifest = $json }
}

# ---------------------------------------------------------------------------
# Serve both, run the app against each, read the log
# ---------------------------------------------------------------------------
$serverScript = Join-Path (Split-Path -Parent $PSScriptRoot) "scripts\serve_updater_release.mjs"
$jobs = @()
foreach ($case in @(@{ name = "bom"; port = $PortBom }, @{ name = "plain"; port = $PortPlain })) {
  $job = Start-Job -ScriptBlock { param($svc, $dir, $port) & node $svc $dir $port 2>&1 } `
    -ArgumentList $serverScript, $trees[$case.name].root, $case.port
  $jobs += $job
  Info ("started " + $case.name + " server as job " + $job.Id + " on " + $case.port)
}

$env:CC_SWITCH_TEST_HOME = $null
$outcomes = @{}
try {
  foreach ($case in @(@{ name = "bom"; port = $PortBom }, @{ name = "plain"; port = $PortPlain })) {
    $url = "http://127.0.0.1:$($case.port)/releases/latest/download/version.json"
    $up = $false
    for ($i = 0; $i -lt 60; $i++) {
      try { $r = Invoke-WebRequest -Uri $url -TimeoutSec 3 -UseBasicParsing -ErrorAction Stop; $up = $true; break }
      catch { Start-Sleep -Milliseconds 250 }
    }
    if (-not $up) { Fail ("server for case " + $case.name + " never came up"); $outcomes[$case.name] = $null; continue }

    $home_ = Join-Path $Work ("home-" + $case.name)
    Remove-Item -LiteralPath $home_ -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $home_ | Out-Null

    $env:CC_SWITCH_TEST_HOME = $home_
    $env:HOME = $home_
    $env:AIMYRIA_UPDATE_TEST_CLOCK = "1"
    $env:AIMYRIA_UPDATE_MANIFEST_URL = $url
    Remove-Item Env:\AIMYRIA_UPDATE_DRYRUN -ErrorAction SilentlyContinue
    try { $proc = Start-Process -FilePath $Exe -PassThru -ErrorAction Stop }
    catch { Fail ("could not launch for case " + $case.name + ": " + $_.Exception.Message); $outcomes[$case.name] = $null; continue }
    finally {
      Remove-Item Env:\AIMYRIA_UPDATE_TEST_CLOCK, Env:\AIMYRIA_UPDATE_MANIFEST_URL, Env:\CC_SWITCH_TEST_HOME, Env:\HOME -ErrorAction SilentlyContinue
    }

    Info ("case " + $case.name + ": launched pid " + $proc.Id + ", waiting ${WaitSeconds}s")
    Start-Sleep -Seconds $WaitSeconds
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue

    $logPath = Join-Path $home_ ".aimyria\logs\aimyria-one.log"
    # The log is held open by the app until it exits, and `Stop-Process -Force`
    # returns before the handle is released. A plain ReadAllText then throws
    # "being used by another process", which -- because the read is inside a
    # try/finally -- silently produced `$outcomes[x] = $null` and the whole case
    # reported "did not run" rather than "the app logged X". Retry with a
    # share-all open so the measurement happens instead of failing quietly.
    $text = ""
    for ($attempt = 0; $attempt -lt 20; $attempt++) {
      try {
        if (Test-Path -LiteralPath $logPath) {
          $fs = [System.IO.File]::Open($logPath, 'Open', 'Read', 'ReadWrite')
          try {
            $sr = New-Object System.IO.StreamReader($fs, [System.Text.Encoding]::UTF8)
            $text = $sr.ReadToEnd()
            $sr.Close()
          } finally { $fs.Close() }
        }
        break
      } catch { Start-Sleep -Milliseconds 250 }
    }
    $parseFail = $text -match 'manifest did not parse'
    $parseOk = $text -match '\[updater\] manifest: version='
    $upToDate = $text -match 'up to date'
    $updateAvail = $text -match 'update available'
    $outcomes[$case.name] = [pscustomobject]@{
      ParseOk = $parseOk; ParseFail = $parseFail; UpToDate = $upToDate; UpdateAvail = $updateAvail; LogPath = $logPath
    }
    Write-Output ("  -- case " + $case.name + ": [updater] lines from the app's own log --")
    ($text -split "`r?`n" | Where-Object { $_ -match '\[updater\]' } | Select-Object -First 8) | ForEach-Object { Info ("      " + $_.Trim()) }
  }
} finally {
  Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($Work, [StringComparison]::OrdinalIgnoreCase) } | Stop-Process -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 1
  foreach ($j in $jobs) { Stop-Job -Job $j -ErrorAction SilentlyContinue; Remove-Job -Job $j -Force -ErrorAction SilentlyContinue }
}

# ---------------------------------------------------------------------------
# Assertions
# ---------------------------------------------------------------------------
Write-Output ""
Write-Output "=== verdict ==="
foreach ($name in @("bom", "plain")) {
  $o = $outcomes[$name]
  if ($null -eq $o) { Fail ("case " + $name + " did not run"); continue }
  # The defect produced exactly this line; it must be gone.
  if ($o.ParseFail) { Fail ("case " + $name + ": the app logged 'manifest did not parse' -- the BOM fix is NOT effective in the binary") }
  else { Pass ("case " + $name + ": the app never logged 'manifest did not parse'") }

  if ($o.ParseOk) { Pass ("case " + $name + ": the app parsed the manifest and logged its fields") }
  else { Fail ("case " + $name + ": no 'manifest: version=' line -- the manifest was never parsed") }

  # And it must have got past parsing into the version decision, whichever way
  # that went. Both outcomes prove the parse succeeded.
  if ($o.UpdateAvail) { Pass ("case " + $name + ": version comparison ran and found an update") }
  elseif ($o.UpToDate) { Pass ("case " + $name + ": version comparison ran and reported up-to-date") }
  else { Fail ("case " + $name + ": parsing did not lead to a version decision") }
}

Write-Output ""
Write-Output "=== summary ==="
$results | ForEach-Object { Write-Output ("  " + $_) }
$failed = @($results | Where-Object { $_ -like "FAIL*" }).Count
Write-Output ("  failures: " + $failed)
Write-Output ("  tree kept for inspection: " + $Work)
exit $(if ($failed -eq 0) { 0 } else { 1 })
