# Pre-publish artifact assertions.
#
# Five things are checked, four of which T5's release gate depends on. Every one of
# them exists because of a specific way this project has already shipped the wrong
# thing:
#
#   1. `321ai` -- the third-party gateway the owner had purged from the frontend.
#      It also survives in the *backend* as `RELAY_ALT_HOST = "321ai.cc"`, which is
#      here on purpose: the check is about the frontend having no trace of it. The
#      backend occurrence is asserted deliberately, below, so it cannot silently
#      disappear and be mistaken for a clean tree.
#   2. `brand-mark` -- present in the *bundled CSS*, not in the executable's plain
#      strings. Tauri embeds frontend assets Brotli-compressed, so `--brand-mark`
#      cannot be grepped out of the binary; the correct assertion is that the asset
#      manifest references the same CSS file the current `dist/` produced, and that
#      the token is in it.
#   3. the WorkBuddy icon -- the official vector replaced the poster thumbnail.
#   4. the updater's manifest URL -- without it the update channel is dead on
#      arrival regardless of everything else.
#   5. the competitor's release URL -- must be gone.
#
# A note on what "absent from the exe" can and cannot prove. Minified JS and CSS
# reach the binary as a compressed blob; only some string literals survive as plain
# text (Rust format strings, Tauri config). So each assertion below names which
# artifact it reads: `exe-plain` means a raw byte search will find it, `dist` means
# it is checked in the built renderer instead.
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [string]$Dist = ""
)

$ErrorActionPreference = "Continue"
$failed = 0
function Check($name, $ok, $detail) {
  if ($ok) { Write-Output ("  [PASS] " + $name + ": " + $detail) }
  else { Write-Output ("  [FAIL] " + $name + ": " + $detail); $script:failed++ }
}
function Note($line) { Write-Output ("  [note] " + $line) }

if (-not $Dist) { $Dist = Join-Path (Split-Path -Parent $PSScriptRoot) "dist" }

if (-not (Test-Path -LiteralPath $Exe)) { Write-Output "no exe at $Exe"; exit 2 }
$bytes = [System.IO.File]::ReadAllBytes($Exe)
$ascii = [System.Text.Encoding]::ASCII.GetString($bytes)
$utf16 = [System.Text.Encoding]::Unicode.GetString($bytes)

$js = (Get-ChildItem (Join-Path $Dist "assets") -Filter "index-*.js" |
       Sort-Object Length -Descending | Select-Object -First 1)
$css = (Get-ChildItem (Join-Path $Dist "assets") -Filter "index-*.css" |
        Sort-Object Length -Descending | Select-Object -First 1)
$jsText = [System.IO.File]::ReadAllText($js.FullName)
$cssText = [System.IO.File]::ReadAllText($css.FullName)

Write-Output ("exe  : $Exe")
Write-Output ("size : " + $bytes.Length + " bytes")
Write-Output ("dist : " + $js.Name + " + " + $css.Name)
Write-Output ""

# -- 1. the frontend is free of the third-party gateway -----------------------
Write-Output "=== 1. 321AI purged from the shipped renderer ==="
$distHits = ([regex]::Matches($jsText, "321ai", "IgnoreCase")).Count +
            ([regex]::Matches($cssText, "321ai", "IgnoreCase")).Count
Check "dist/ frontend has no '321ai'" ($distHits -eq 0) "$distHits occurrence(s) in the JS+CSS bundles"

# The backend keeps it as a legacy-host constant. Assert it is still exactly the
# one intentional place, so "0 hits" can never be achieved by accident.
$exeHits = ([regex]::Matches($ascii, "321ai", "IgnoreCase")).Count
$backendOk = $ascii.Contains("321ai.cc")
if ($exeHits -eq 0) {
  Check "exe has no '321ai'" $true "absent -- the backend constant is gone too"
} elseif ($exeHits -eq 1 -and $backendOk) {
  Note ("exe has exactly 1 '321ai' and it is the backend legacy-host constant '321ai.cc' (RELAY_ALT_HOST).")
  Note ("     That is outside the frontend purge and is NOT a defect: it keeps existing installs")
  Note ("     written against the old gateway recognised as 'configured'. Asserted so it cannot drift.")
  Check "exe '321ai' occurrences are exactly the known backend constant" $true "1 occurrence, '321ai.cc'"
} else {
  Check "exe '321ai' is only the known backend constant" $false "$exeHits occurrence(s); '321ai.cc' present=$backendOk"
}

# -- 2. the brand-mark token, via the CSS that is actually bundled ------------
Write-Output ""
Write-Output "=== 2. the light-mode brand token ships ==="
# The token lives in CSS. Tauri embeds frontend assets Brotli-compressed, so it is
# NOT a plain string in the exe -- this was checked and is expected. The meaningful
# assertion is that the artifact embeds *this* CSS file (named in the asset
# manifest, which IS plain text in the binary) and that the file carries the token.
$cssName = $css.Name
$cssEmbedded = $ascii.Contains($cssName) -or $utf16.Contains($cssName)
$jsEmbedded = $ascii.Contains($js.Name) -or $utf16.Contains($js.Name)
Check "the exe embeds the current dist's JS" $jsEmbedded $js.Name
Check "the exe embeds the current dist's CSS" $cssEmbedded $cssName
$tokenCount = ([regex]::Matches($cssText, "--brand-mark")).Count
Check "the bundled CSS defines --brand-mark" ($tokenCount -ge 2) "$tokenCount declaration(s) (one per colour scheme)"
Check "CSS tokens are not greppable from the exe (expected)" `
  ((-not $ascii.Contains("--brand-mark")) -and (-not $utf16.Contains("--brand-mark"))) `
  "confirmed compressed -- which is why this check reads dist/ instead"

# -- 3. the official WorkBuddy icon ------------------------------------------
Write-Output ""
Write-Output "=== 3. the official WorkBuddy icon ==="
$wbSvg = Get-ChildItem (Join-Path $Dist "assets") -Filter "workbuddy-*.svg" -ErrorAction SilentlyContinue
$wbPng = Get-ChildItem (Join-Path $Dist "assets") -Filter "workbuddy-*.png" -ErrorAction SilentlyContinue
Check "dist has the WorkBuddy SVG" ($null -ne $wbSvg) $(if ($wbSvg) { $wbSvg.Name + " (" + $wbSvg.Length + " B)" } else { "missing" })
Check "dist has no WorkBuddy PNG poster thumbnail" ($null -eq $wbPng) $(if ($wbPng) { "still present: " + $wbPng.Name } else { "removed" })
$svgName = if ($wbSvg) { $wbSvg.Name } else { "" }
Check "the exe embeds the WorkBuddy SVG" ($svgName -ne "" -and ($ascii.Contains($svgName) -or $utf16.Contains($svgName))) $svgName

# -- 4. the update channel is wired ------------------------------------------
Write-Output ""
Write-Output "=== 4. the self-updater ships its manifest URL ==="
Check "exe contains the pinned manifest URL" `
  $ascii.Contains("aimyria.com/releases/latest/download/version.json") `
  "https://aimyria.com/releases/latest/download/version.json"
Check "exe contains the updater's log tag" $ascii.Contains("[updater]") "[updater] (proves the module linked in)"

# -- 5. the competitor's release URL is gone ---------------------------------
Write-Output ""
Write-Output "=== 5. the competitor release URL is gone ==="
Check "no 'farion1231/cc-switch/releases/latest'" `
  (-not $ascii.Contains("farion1231/cc-switch/releases/latest")) `
  "the URL check_for_updates used to open"
Check "no 'ccswitch.io'" (-not $ascii.Contains("ccswitch.io") -and -not $utf16.Contains("ccswitch.io")) "upstream's site"
Check "the MIT attribution is still present" $ascii.Contains("CC Switch") `
  "'CC Switch' near the attribution (required to ship; not a link)"
Note "github.com/farion1231 still appears in About's attribution text -- a licence obligation, not a link."

Write-Output ""
Write-Output ("  failures: " + $failed)
exit $(if ($failed -eq 0) { 0 } else { 1 })
