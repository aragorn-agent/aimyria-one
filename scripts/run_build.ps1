# Run a build and guarantee the log ends with a verdict.
#
# WHY THIS WRAPPER EXISTS
#
# An earlier attempt logged the build by piping it through `Tee-Object` inside a
# background job. That job died mid-build and left a log whose last line was
# "building - a full first build, expect ~10 min": no error, no exit code, and a
# stale artifact on disk. From the outside that is indistinguishable from a build
# still running, so the next person to look has to guess, and the natural guess is
# "it is probably still going". It was not.
#
# A build log's last line is load-bearing, so it is written by this script rather
# than by the build. The verdict is appended in a `finally`, which runs whether the
# build succeeds, fails, or is killed, and the exit code is propagated unchanged.
#
# Usage:
#   pwsh -File scripts\run_build.ps1 -Log .build-v3.log [-Test]
param(
  [string]$Repo = (Split-Path -Parent $PSScriptRoot),
  [string]$Log = ".build.log",
  [string]$TargetDir = "E:\target-cache\aimyria-v3",
  [switch]$Test
)

$ErrorActionPreference = "Continue"
$logPath = if ([System.IO.Path]::IsPathRooted($Log)) { $Log } else { Join-Path $Repo $Log }
$startedAt = Get-Date
$exitCode = $null
$verdict = "INCOMPLETE"

function Note($line) { Write-Output $line; Add-Content -LiteralPath $logPath -Value $line }

Set-Content -LiteralPath $logPath -Value ("# build started {0}" -f $startedAt.ToString("yyyy-MM-dd HH:mm:ss"))

try {
  $args = @{
    Vcvars      = "D:\VSBuildTools\VC\Auxiliary\Build\vcvars64.bat"
    ManifestDir = (Join-Path $Repo "src-tauri")
    TargetDir   = $TargetDir
    StaticCrt   = $true
  }
  if ($Test) { $args.Test = $true }

  # Stream line by line rather than buffering the lot: a build that hangs is then
  # visible in the log as "last line written 8 minutes ago" instead of as silence.
  & (Join-Path $Repo "scripts\build_msvc.ps1") @args 2>&1 |
    ForEach-Object { $_ | Out-String -Stream } |
    ForEach-Object {
      if ($_ -ne "") { Add-Content -LiteralPath $logPath -Value $_ }
      Write-Output $_
    }
  $exitCode = $LASTEXITCODE
  $verdict = if ($exitCode -eq 0) { "OK" } else { "FAIL" }
} catch {
  $exitCode = 1
  $verdict = "EXCEPTION"
  Note ("!! exception: " + $_.Exception.Message)
} finally {
  $elapsed = [Math]::Round(((Get-Date) - $startedAt).TotalSeconds, 1)
  $final = "BUILD VERDICT: {0} exit={1} elapsed={2}s finished={3}" -f `
    $verdict, $(if ($null -eq $exitCode) { "unknown" } else { $exitCode }), $elapsed, `
    (Get-Date).ToString("yyyy-MM-dd HH:mm:ss")
  Add-Content -LiteralPath $logPath -Value $final
  Write-Output $final
}

if ($null -eq $exitCode) { exit 1 } else { exit $exitCode }
