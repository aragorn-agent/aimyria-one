# Stage the freshly built v1.0.3 as a local release the installer can consume.
#
# The public site still serves the previous build, so installing from
# https://aimyria.com would put the buggy binary back and make the reinstall test
# measure the wrong thing. This stages the v1.0.3 bytes locally, computes their
# checksum the same way package.ps1 does, and writes a version.json so both
# quickstart.ps1 (which verifies .sha256) and the updater (which reads
# version.json) can be pointed at it.
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [Parameter(Mandatory = $true)][string]$OutDir,
  [string]$Version = "1.0.3",
  [string]$BaseUrl = "http://127.0.0.1:8799"
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$dst = Join-Path $OutDir 'AimyriaOne-x64.exe'
Copy-Item -LiteralPath $Exe -Destination $dst -Force
$sha = (Get-FileHash -LiteralPath $dst -Algorithm SHA256).Hash.ToLower()
"$sha  AimyriaOne-x64.exe" | Set-Content -LiteralPath "$dst.sha256" -Encoding ascii -NoNewline

$manifest = [ordered]@{
  version = $Version
  url     = "$BaseUrl/releases/latest/download/AimyriaOne-x64.exe"
  sha256  = $sha
  notes   = "silent startup: protocol registration no longer spawns reg.exe"
  pubDate = (Get-Date).ToString("yyyy-MM-ddTHH:mm:sszzz")
}
$manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $OutDir 'version.json') -Encoding utf8

Write-Output ("staged  " + $dst + "  " + (Get-Item $dst).Length + " B")
Write-Output ("sha256  " + $sha)
Write-Output ("version " + $Version)
Get-ChildItem $OutDir -File | ForEach-Object { Write-Output ("  " + $_.Name + "  " + $_.Length + " B") }
