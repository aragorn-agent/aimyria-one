# Version discipline: the three files that must agree.
#
# WHY THIS IS A CHECK AND NOT A NOTE IN A README
#
# The self-updater compares the manifest against `env!("CARGO_PKG_VERSION")`, which
# comes from `src-tauri/Cargo.toml`. If that file is not bumped, the comparison
# says "already up to date" forever and the release simply never reaches anyone --
# silently, because a silent updater's whole job is to say nothing. The other two
# places are what the UI and the bundle report, and a build where they disagree
# shows the user a version that is not the one the updater is using.
#
# So the invariant is asserted, not documented. Run it before publishing.
param(
  [string]$Repo = (Split-Path -Parent $PSScriptRoot),
  [string]$Expect = ""
)

$ErrorActionPreference = "Continue"
$failed = 0
function Check($name, $ok, $detail) {
  if ($ok) { Write-Output ("  [PASS] " + $name + ": " + $detail) }
  else { Write-Output ("  [FAIL] " + $name + ": " + $detail); $script:failed++ }
}

$cargo = Join-Path $Repo "src-tauri\Cargo.toml"
$tauri = Join-Path $Repo "src-tauri\tauri.conf.json"
$pkg = Join-Path $Repo "package.json"

# Read everything as UTF-8 explicitly.
#
# Windows PowerShell 5.1's `Get-Content -Raw` without `-Encoding` decodes a file
# using the *system ANSI codepage* when it sees no BOM. Both of these files carry
# Chinese (the product/description strings), so on this machine -- whose ANSI
# codepage is GBK -- the text arrives as mojibake and `ConvertFrom-Json` then throws
# "Invalid object passed in, ':' or '}' expected" against a file that is perfectly
# valid UTF-8 JSON. The first version of this script reported exactly that, and it
# very nearly sent me hunting for a corrupted config that did not exist. Read as
# UTF-8, or do not claim to have parsed a file whose content is non-ASCII.
$cargoText = Get-Content -LiteralPath $cargo -Raw -Encoding UTF8
$tauriText = Get-Content -LiteralPath $tauri -Raw -Encoding UTF8
$pkgText = Get-Content -LiteralPath $pkg -Raw -Encoding UTF8

# Only the `[package]` table's version, not the first `version =` anywhere: the
# dependency list is full of them and a loose match would read the wrong one.
$cargoSection = ($cargoText -split '^\[')[1]
$cargoVersion = ([regex]::Match($cargoSection, '(?m)^\s*version\s*=\s*"([^"]+)"')).Groups[1].Value

$tauriVersion = ""
try { $tauriVersion = ($tauriText | ConvertFrom-Json).version }
catch {
  Write-Output ("  [FAIL] src-tauri/tauri.conf.json is not valid JSON: " + $_.Exception.Message)
  $script:failed++
}

$pkgVersion = ($pkgText | ConvertFrom-Json).version

Write-Output "=== version discipline ==="
Write-Output ("  src-tauri/Cargo.toml      : " + $cargoVersion + "   <- env!(CARGO_PKG_VERSION), what the updater compares against")
Write-Output ("  src-tauri/tauri.conf.json : " + $tauriVersion)
Write-Output ("  package.json              : " + $pkgVersion)

Check "Cargo.toml == tauri.conf.json" ($cargoVersion -eq $tauriVersion) "$cargoVersion vs $tauriVersion"
Check "Cargo.toml == package.json" ($cargoVersion -eq $pkgVersion) "$cargoVersion vs $pkgVersion"

# A version the updater cannot compare is a version that never updates. The
# manifest side is checked by the publisher; this is the installed side.
$numeric = [regex]::IsMatch($cargoVersion, '^\d+\.\d+\.\d+$')
Check "three dot-separated numbers" $numeric "'$cargoVersion' must match ^\d+\.\d+\.\d+$ (no 'v' prefix, no '-beta')"

if ($Expect) { Check "matches -Expect $Expect" ($cargoVersion -eq $Expect) "got $cargoVersion" }

# The manifest URL and what T5 publishes are two halves of one contract; a
# mismatch here is a channel that is up on both sides and connected on neither.
$updater = Join-Path $Repo "src-tauri\src\updater.rs"
$pinned = ([regex]::Match((Get-Content -LiteralPath $updater -Raw),
  '(?s)DEFAULT_MANIFEST_URL:\s*&str\s*=\s*"([^"]+)"')).Groups[1].Value
$expected = "https://aimyria.com/releases/latest/download/version.json"
Check "pinned manifest URL" ($pinned -eq $expected) $pinned

Write-Output ("  failures: " + $failed)
exit $(if ($failed -eq 0) { 0 } else { 1 })
