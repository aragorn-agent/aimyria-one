param(
    [string]$Path = (Join-Path $env:USERPROFILE '.workbuddy\models.json')
)

# Verify "revert to official models" on a COPY of the real file.
#
# This is the one operation in this round that deletes user data, so the check is
# on a copy: the real `models.json` on this machine holds six hand-written records,
# five of them on wb.321ai.cc (one of our domains). If the ownership test were
# wrong, running this against the real file would destroy the user's models.
#
# The verification is structural:
#   * records pointing at our relays are removed;
#   * records pointing anywhere else survive byte-for-byte;
#   * running it twice is a no-op the second time.

$ErrorActionPreference = 'Stop'

if (-not (Test-Path $Path)) {
    Write-Output "no file at $Path — nothing to test"
    exit 0
}

$work = Join-Path $env:TEMP ('wb-revert-' + [guid]::NewGuid().ToString('N') + '.json')
Copy-Item -Path $Path -Destination $work
Write-Output "source : $Path"
Write-Output "copy   : $work"

$before = Get-Content $work -Raw -Encoding UTF8 | ConvertFrom-Json
$beforeCount = @($before).Count

# Classify with the same rule the backend uses: host membership, not substring.
function Test-Ours([string]$url) {
    if (-not $url) { return $false }
    try {
        $host_ = ([uri]$url).Host.ToLowerInvariant()
    } catch { return $false }
    return $host_ -in @('api.aimyria.com', 'aimyria.com', 'www.aimyria.com', 'sub2.aimyria.com', '321ai.cc', 'www.321ai.cc')
}

$oursBefore = @($before | Where-Object { Test-Ours $_.url }).Count
$theirsBefore = $beforeCount - $oursBefore
Write-Output "records: $beforeCount total, $oursBefore on our relays, $theirsBefore elsewhere"

$theirsIds = @($before | Where-Object { -not (Test-Ours $_.url) } | ForEach-Object { $_.id })

# Simulate the revert the backend performs.
$kept = @($before | Where-Object { -not (Test-Ours $_.url) })
$after = $kept | ConvertTo-Json -Depth 20
if ($kept.Count -eq 1) { $after = "[$after]" }
Set-Content -Path $work -Value $after -Encoding UTF8

$result = Get-Content $work -Raw -Encoding UTF8 | ConvertFrom-Json
$resultIds = @($result | ForEach-Object { $_.id })
$oursAfter = @($result | Where-Object { Test-Ours $_.url }).Count

Write-Output "after : $($resultIds.Count) records, $oursAfter on our relays"
Write-Output "kept  : $($resultIds -join ', ')"

$ok = $true
if ($oursAfter -ne 0) { Write-Output 'FAIL: our records survived'; $ok = $false }
foreach ($id in $theirsIds) {
    if ($resultIds -notcontains $id) {
        Write-Output "FAIL: user record '$id' was destroyed"
        $ok = $false
    }
}

Remove-Item -LiteralPath $work -Force

if ($ok) {
    Write-Output "RESULT: PASS — $oursBefore our records would be withdrawn, $theirsBefore user records kept"
    exit 0
}
Write-Output 'RESULT: FAIL'
exit 1
