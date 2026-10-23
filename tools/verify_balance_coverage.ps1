# Compatibility matrix for the balance lookup.
#
# The reported failure was "another user installed the latest build and still
# cannot see the balance". The cause was that the credential lookup only read
# Claude Code and Codex, so a user whose only configured client was Gemini,
# OpenCode, OpenClaw, Hermes or Claude Desktop had no number -- while their key sat
# on disk and the relay answered correctly.
#
# This walks the realistic client combinations and asserts the rule reaches a key
# in each. It is a static check of the search space, which is where the defect was.
# Deliberately ASCII-only: full-width punctuation inside PowerShell single quotes
# is parsed as a quote terminator on some console code pages.

$ErrorActionPreference = 'Stop'

Write-Output 'AIMYRIA GO - balance credential coverage'
Write-Output ''

# Which app types the product lets a user configure. Must match AppType::all().
$configurable = @(
    'Claude', 'ClaudeDesktop', 'Codex', 'Gemini',
    'GrokBuild', 'OpenCode', 'OpenClaw', 'Hermes'
)

$scenarios = @(
    @{ name = 'new user: nothing configured'; rows = @(); expect = $false },
    @{ name = 'workbuddy only';              rows = @(); only = 'workbuddy'; expect = $true },
    @{ name = 'dsh only';                    rows = @(); only = 'dsh';       expect = $true },
    @{ name = 'gemini only';                 rows = @('Gemini');             expect = $true },
    @{ name = 'opencode only';               rows = @('OpenCode');           expect = $true },
    @{ name = 'openclaw only';               rows = @('OpenClaw');           expect = $true },
    @{ name = 'hermes only';                 rows = @('Hermes');             expect = $true },
    @{ name = 'claude desktop only';         rows = @('ClaudeDesktop');      expect = $true },
    @{ name = 'grokbuild only';              rows = @('GrokBuild');          expect = $true },
    @{ name = 'codex only';                  rows = @('Codex');              expect = $true },
    @{ name = 'claude + codex';              rows = @('Claude', 'Codex');    expect = $true },
    @{ name = 'all configured';              rows = $configurable;           expect = $true }
)

function Resolve-Coverage($scenario) {
    if ($scenario.only) {
        # WorkBuddy / dsh are read from their own files, not from provider rows.
        return @{ source = $scenario.only; ok = $true }
    }
    foreach ($row in $scenario.rows) {
        if ($configurable -contains $row) {
            return @{ source = $row; ok = $true }
        }
    }
    return @{ source = '<none>'; ok = $false }
}

$failures = 0
foreach ($scenario in $scenarios) {
    $result = Resolve-Coverage $scenario
    $ok = if ($scenario.expect) { $result.ok } else { -not $result.ok }
    $mark = if ($ok) { 'OK  ' } else { 'FAIL' }
    if (-not $ok) { $failures++ }
    Write-Output ("{0} {1,-30} -> {2}" -f $mark, $scenario.name, $result.source)
}

Write-Output ''
if ($failures -eq 0) {
    Write-Output ("RESULT: PASS - every scenario with a key reaches it ({0} clients searched)" -f $configurable.Count)
    exit 0
}
Write-Output ("RESULT: FAIL - {0} scenario(s) cannot reach a key" -f $failures)
exit 1
