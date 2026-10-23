param([int]$Port = 9333)

# Ask the running WebView2 what it rendered.
#
# This is the check that was missing from every earlier round: "the binary contains
# the new bundle" is a claim about bytes, not about what the user sees. A build can
# embed the right JS and still render an old page if the app is a stale process, if
# a cached asset is served, or if the frontend throws before mounting.
#
# So: connect to the DevTools protocol and read the actual DOM text.

$ErrorActionPreference = 'Stop'

try {
    $targets = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/json" -TimeoutSec 5
} catch {
    Write-Output "FAIL: no devtools endpoint on port $Port - $($_.Exception.Message)"
    exit 1
}

$page = $targets | Where-Object { $_.type -eq 'page' } | Select-Object -First 1
if (-not $page) { Write-Output 'FAIL: no page target'; exit 1 }

Write-Output "page: $($page.title) $($page.url)"
$wsUrl = $page.webSocketDebuggerUrl

Add-Type -AssemblyName System.Net.WebSockets.Client
$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$ct = [System.Threading.CancellationToken]::None
$ws.ConnectAsync([Uri]$wsUrl, $ct).Wait(5000) | Out-Null

function Send-Eval([string]$expr) {
    $payload = @{ id = 1; method = 'Runtime.evaluate'; params = @{ expression = $expr; returnByValue = $true } } | ConvertTo-Json -Depth 5 -Compress
    $bytes = [Text.Encoding]::UTF8.GetBytes($payload)
    $ws.SendAsync([ArraySegment[byte]]::new($bytes), 'Text', $true, $ct).Wait(5000) | Out-Null

    $buffer = New-Object byte[] 262144
    $sb = New-Object Text.StringBuilder
    do {
        $result = $ws.ReceiveAsync([ArraySegment[byte]]::new($buffer), $ct)
        $result.Wait(10000) | Out-Null
        $sb.Append([Text.Encoding]::UTF8.GetString($buffer, 0, $result.Result.Count)) | Out-Null
    } while (-not $result.Result.EndOfMessage)

    return $sb.ToString()
}

# The left navigation is built from the same data as the pages, so its labels are
# the cheapest proof that the new bundle is the one running.
$nav = Send-Eval "Array.from(document.querySelectorAll('nav a, nav button, [role=navigation] a')).map(e=>e.textContent.trim()).filter(Boolean).join('|')"
Write-Output "nav raw: $nav"

$body = Send-Eval "document.body.innerText.slice(0,400)"
Write-Output "body head: $body"

$ws.Dispose()
