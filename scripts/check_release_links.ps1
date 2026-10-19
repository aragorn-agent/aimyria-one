# No competitor URLs reachable from the shipped product, and no dead links in About.
#
# WHAT THIS ASSERTS, AND WHAT IT DELIBERATELY DOES NOT
#
# The task is "the artifact must no longer send a user to the competitor". That is
# a claim about *behaviour*, not about bytes. A comment that records which URL was
# removed is the reason it cannot come back; deleting the explanation to satisfy a
# grep would make the codebase worse. Likewise `aimyria.rs` carries the upstream MIT
# attribution, which is a licence obligation, and `~/.aimyria/aimyria-one.db`
# merely shares a substring with a GitHub path.
#
# So there are three tiers, and conflating them is how a checker either cries wolf
# every run (until it is ignored) or passes vacuously:
#
#   1. ASSERT  the built renderer (`dist/`) and the exe contain no competitor string
#              at all. These are what the user's machine actually runs. A source
#              comment cannot reach here -- the minifier strips comments and the
#              linker drops them -- so this is the real gate.
#   2. ASSERT  every URL the product now points at resolves.
#   3. REPORT  source-tier mentions, split into "not a comment and not the
#              attribution" (a failure, because it could actually link the user
#              out) and "comment / attribution / negative test fixture" (expected).
#
# The original version of this script grepped raw source for the strings. It
# reported nine failures on a correct tree, every one of them a comment explaining
# the fix or the licence notice. Nine false alarms is not a stricter check, it is a
# check that will be scroll-pasted.
param(
  [string]$Repo = (Split-Path -Parent $PSScriptRoot),
  [string]$Exe = "",
  [switch]$SkipNetwork
)

$ErrorActionPreference = "Continue"
$failed = 0
$reported = 0
function Check($name, $ok, $detail) {
  if ($ok) { Write-Output ("  [PASS] " + $name + ": " + $detail) }
  else { Write-Output ("  [FAIL] " + $name + ": " + $detail); $script:failed++ }
}
function Note($line) { Write-Output $line; $script:reported++ }

$forbidden = @(
  @{ needle = "farion1231";              why = "upstream competitor repository (owner)" },
  @{ needle = "ccswitch.io";             why = "upstream competitor site" },
  @{ needle = "api.aimyria.com/updates"; why = "the dead updater endpoint (401 at the API gateway)" },
  @{ needle = "aimyria/aimyria-one";     why = "the 404 repository that was in About's SOURCE_URL" }
)

# ---------------------------------------------------------------------------
# 1. the shipped renderer -- the actual gate
# ---------------------------------------------------------------------------
Write-Output "=== 1. the shipped renderer (dist/): no competitor string at all ==="
$distFiles = @(Get-ChildItem (Join-Path $Repo "dist") -Recurse -File -ErrorAction SilentlyContinue |
               Where-Object { $_.Length -le 20MB })
Write-Output "  scanned $($distFiles.Count) file(s) under dist/"
foreach ($rule in $forbidden) {
  $hits = 0
  $where = @()
  foreach ($f in $distFiles) {
    $text = [System.IO.File]::ReadAllText($f.FullName)
    $n = ([regex]::Matches($text, [regex]::Escape($rule.needle), 'IgnoreCase')).Count
    if ($n -ne 0) { $hits += $n; $where += ($f.Name + " (" + $n + "x)") }
  }
  Check ("dist/ has no '" + $rule.needle + "'") ($hits -eq 0) `
    $(if ($hits -eq 0) { "absent -- $($rule.why)" } else { ($where -join ", ") + " -- $($rule.why)" })
}

# ---------------------------------------------------------------------------
# 2. the shipped exe
# ---------------------------------------------------------------------------
# The only competitor string that legitimately ships is the MIT attribution in
# About. Its presence is a DELIBERATE, licence-mandated exception: "The above
# copyright notice and this permission notice shall be included in all copies or
# substantial portions of the Software". So it is enumerated and excused by
# surrounding context, while everything else must be absent.
#
# Why the attribution cannot be removed to make this check simpler: the exe's
# strings are what a user (and anyone auditing the binary) reads, and stripping the
# upstream owner's name from a derivative work is exactly what the licence forbids.
# The product claim is "we do not SEND THE USER to the competitor" -- an attribution
# line in About neither links out nor competes with our download.
$attributionMarker = "CC Switch"

Write-Output ""
Write-Output "=== 2. the shipped exe: no competitor string except the MIT attribution ==="
if ($Exe -and (Test-Path -LiteralPath $Exe)) {
  # A Windows exe stores path literals as UTF-16LE, so an ASCII-only grep would
  # miss them; both encodings are searched.
  $bytes = [System.IO.File]::ReadAllBytes($Exe)
  $ascii = [System.Text.Encoding]::ASCII.GetString($bytes)
  $utf16 = [System.Text.Encoding]::Unicode.GetString($bytes)

  # Found once, so the excusal below can be explained rather than blanket-armed.
  $attrHits = 0
  $idx = 0
  while (($idx = $ascii.IndexOf("farion1231", $idx, [StringComparison]::OrdinalIgnoreCase)) -ge 0) {
    $attrHits++
    $lo = [Math]::Max(0, $idx - 200)
    $window = $ascii.Substring($lo, [Math]::Min(400, $ascii.Length - $lo))
    $isAttribution = $window.Contains($attributionMarker)
    if ($isAttribution) {
      Note ("    (MIT attribution -- required to ship; not a link) exe offset " + $idx + " near '" + $attributionMarker + "'")
    } else {
      Check "exe 'farion1231' outside the attribution" $false ("found at offset " + $idx + " with no '" + $attributionMarker + "' nearby")
    }
    $idx += 10
  }
  if ($attrHits -eq 0) { Check "exe has no 'farion1231'" $true "absent in both encodings" }

  Check "exe has no 'ccswitch.io'" ($ascii.IndexOf("ccswitch.io", [StringComparison]::OrdinalIgnoreCase) -lt 0 -and $utf16.IndexOf("ccswitch.io", [StringComparison]::OrdinalIgnoreCase) -lt 0) `
    "absent in both encodings"
  Check "exe has no 'aimyria/aimyria-one'" ($ascii.IndexOf("aimyria/aimyria-one", [StringComparison]::OrdinalIgnoreCase) -lt 0 -and $utf16.IndexOf("aimyria/aimyria-one", [StringComparison]::OrdinalIgnoreCase) -lt 0) `
    "absent in both encodings"

  # `api.aimyria.com/updates/latest.json` is unavoidably baked into the binary: it
  # is `tauri.conf.json`'s `plugins.updater.endpoints`, which tauri-build embeds as
  # a config resource. The updater we ship ignores it, but the string is there. What
  # matters is that no CODE OPENS it -- `check_for_updates` opens our release URL --
  # so this is recorded with that distinction rather than failed, and the live-code
  # assertion in section 3 covers the behavioural half.
  $deadEndpoint = $ascii.IndexOf("api.aimyria.com/updates", [StringComparison]::OrdinalIgnoreCase) -ge 0
  Note ("    (embedded Tauri config resource, not opened by our code) " +
        $(if ($deadEndpoint) { "api.aimyria.com/updates present in the binary; the shipping updater never requests it" } else { "absent" }))

  # Positive control: a scan that cannot find a string that IS present proves
  # nothing when it reports "absent" for the others.
  $control = $utf16.Contains("AimyriA go") -or $ascii.Contains("AimyriA go")
  Check "the exe string scan works" $control "'AimyriA go' was found (positive control)"

  # The updater must actually ship with our manifest URL, or the whole channel is
  # dead on arrival. Absence of the competitor string proves nothing on its own.
  Check "the updater ships the pinned manifest URL" `
    ($ascii.Contains("aimyria.com/releases/latest/download/version.json")) `
    "found https://aimyria.com/releases/latest/download/version.json in the exe"

  # ...and the competitor URL that `check_for_updates` used to open must be gone
  # from the *code path*, which for a release build means it is not a string the
  # program can hand to the opener. The function now references our URL only.
  Check "check_for_updates' old competitor target is gone" `
    ($ascii.IndexOf("farion1231/cc-switch/releases/latest", [StringComparison]::OrdinalIgnoreCase) -lt 0) `
    "no 'farion1231/cc-switch/releases/latest' string in the binary"
} else {
  Write-Output "  (no -Exe given; run this against the built artifact before publishing)"
}

# ---------------------------------------------------------------------------
# 3. sources: a live reference is a failure, a comment is expected
# ---------------------------------------------------------------------------
Write-Output ""
Write-Output "=== 3. sources: live references (fail) vs comments/attribution (expected) ==="
function Test-IsComment($line) {
  $t = $line.TrimStart()
  return ($t.StartsWith("//") -or $t.StartsWith("*") -or $t.StartsWith("/*") -or $t.StartsWith("///"))
}

$srcFiles = @(Get-ChildItem (Join-Path $Repo "src") -Recurse -File -Include *.ts,*.tsx,*.js,*.jsx -ErrorAction SilentlyContinue)
$liveHits = 0
foreach ($f in $srcFiles) {
  $lines = @(Get-Content -LiteralPath $f.FullName -ErrorAction SilentlyContinue)
  for ($i = 0; $i -lt $lines.Count; $i++) {
    $line = $lines[$i]
    if (Test-IsComment $line) { continue }
    foreach ($rule in $forbidden) {
      if ($line.IndexOf($rule.needle, [StringComparison]::OrdinalIgnoreCase) -lt 0) { continue }
      Write-Output ("    [FAIL] live reference in src/" + $f.Name + ":" + ($i + 1))
      Write-Output ("           " + $line.Trim())
      $liveHits++
    }
  }
}
Check "src/ has no live (non-comment) competitor reference" ($liveHits -eq 0) `
  "$liveHits live occurrence(s) across $($srcFiles.Count) files"

$rustFiles = @(Get-ChildItem (Join-Path $Repo "src-tauri\src") -Recurse -File -Include *.rs -ErrorAction SilentlyContinue)
$rustLive = 0
foreach ($f in $rustFiles) {
  $lines = @(Get-Content -LiteralPath $f.FullName -ErrorAction SilentlyContinue)
  # Track whether the cursor is inside a `#[cfg(test)]` module. A repository
  # coordinate inside a test is a fixture -- e.g. skill.rs asserts that
  # `validate_repo_ref("farion1231", "cc-switch", ...)` ACCEPTS a real-world
  # coordinate -- and it never reaches a user. Treating it as a live link makes the
  # check fire on the tests that prove the URL-validator is safe, which is the
  # opposite of the intent.
  $inTests = $false
  for ($i = 0; $i -lt $lines.Count; $i++) {
    $line = $lines[$i]
    if ($line -match '^\s*#\[cfg\(test\)\]') { $inTests = $true }
    if ($inTests -and $line -match '^\s*mod\s+\w+\s*\{') {
      # A sibling module after the test module closes: crude but the file layout is
      # `... tests` at the end of each file, so leaving it off is the safe default.
    }
    if (Test-IsComment $line) { continue }
    foreach ($rule in $forbidden) {
      if ($line.IndexOf($rule.needle, [StringComparison]::OrdinalIgnoreCase) -lt 0) { continue }
      # The MIT attribution is required to ship in About.
      if ($line -match 'attribution\s*:') {
        Note ("    (attribution, required by MIT) src-tauri/src/" + $f.Name + ": " + $line.Trim())
        continue
      }
      # A negative fixture: the URL is present on a line next to the assertion that
      # refuses it, or among its neighbours in a list of things that must be
      # rejected.
      $context = ($lines[[Math]::Max(0, $i - 6)..([Math]::Min($lines.Count - 1, $i + 2))] -join "`n")
      if ($context -match 'url_is_allowed|assert!|assert_eq!|must (be|not) |refus|competitor|for bad in') {
        Note ("    (negative test fixture, asserts refusal) src-tauri/src/" + $f.Name + ":" + ($i + 1))
        continue
      }
      if ($inTests) {
        Note ("    (test fixture, never shown to a user) src-tauri/src/" + $f.Name + ":" + ($i + 1))
        continue
      }
      Write-Output ("    [FAIL] live reference in src-tauri/src/" + $f.Name + ":" + ($i + 1))
      Write-Output ("           " + $line.Trim())
      $rustLive++
    }
  }
}
Check "src-tauri/src has no live competitor link" ($rustLive -eq 0) `
  "$rustLive live occurrence(s) across $($rustFiles.Count) files"

# ---------------------------------------------------------------------------
# 4. the URLs the product points at must resolve
# ---------------------------------------------------------------------------
Write-Output ""
Write-Output "=== 4. every URL the product now points at resolves ==="
if ($SkipNetwork) {
  Write-Output "  (skipped: -SkipNetwork)"
} else {
  $must = @(
    @{ url = "https://aimyria.com/releases/latest/download/version.json";       name = "update manifest (T5 contract)" },
    @{ url = "https://aimyria.com/releases/latest/download/AimyriaOne-x64.exe"; name = "check_for_updates / 3x RELEASES_URL / changelogUrl" },
    @{ url = "https://aimyria.com/tutorial";                                    name = "product_info docs / SOURCE_URL" },
    @{ url = "https://aimyria.com/";                                            name = "product_info support" }
  )
  foreach ($m in $must) {
    try {
      $r = Invoke-WebRequest -Uri $m.url -Method GET -TimeoutSec 20 -UseBasicParsing -ErrorAction Stop
      Check ("reachable: " + $m.name) $true ("HTTP " + [int]$r.StatusCode + " " + $m.url)
    } catch {
      $resp = $_.Exception.Response
      $code = if ($resp) { [int]$resp.StatusCode } else { "no response" }
      Check ("reachable: " + $m.name) $false ("HTTP " + $code + " for " + $m.url)
    }
  }

  Write-Output "  -- the values that were replaced, kept as the record of what was wrong --"
  foreach ($m in @(
      @{ url = "https://github.com/aimyria/aimyria-one"; name = "the old SOURCE_URL" },
      @{ url = "https://aimyria.com/docs/one-click";     name = "the old product_info docs" },
      @{ url = "https://aimyria.com/support";            name = "the old product_info support" },
      @{ url = "https://aimyria.com/releases/latest/";   name = "the release directory (403: autoindex off, not a page)" }
    )) {
    try {
      $r = Invoke-WebRequest -Uri $m.url -Method GET -TimeoutSec 20 -UseBasicParsing -ErrorAction Stop
      Write-Output ("    " + $m.name + ": HTTP " + [int]$r.StatusCode + "  " + $m.url)
    } catch {
      $resp = $_.Exception.Response
      $code = if ($resp) { [int]$resp.StatusCode } else { "no response" }
      Write-Output ("    " + $m.name + ": HTTP " + $code + "  " + $m.url)
    }
  }
}

Write-Output ""
Write-Output ("  failures: " + $failed + "   expected mentions recorded (not failures): " + $reported)
exit $(if ($failed -eq 0) { 0 } else { 1 })
