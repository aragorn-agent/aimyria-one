# Deep-link end-to-end check.
#
# "一键配置" means: a browser opens `aimyria://…`, this app receives it, and the
# client's config file on disk changes. Unit tests cover the writer; only this
# covers the path a user actually walks.
#
# It prints what changed and fails loudly, rather than asserting on a file the
# script then leaves modified. Run it on a machine where you are willing to have
# your client configs rewritten — or use -WhatIf for a dry run.
#
#   powershell -File scripts/test-deeplink.ps1
#   powershell -File scripts/test-deeplink.ps1 -App claude -WhatIf

[CmdletBinding()]
param(
    # claude | codex | gemini | opencode | openclaw
    [ValidateSet('claude', 'codex', 'gemini', 'opencode', 'openclaw')]
    [string]$App = 'claude',
    # Print the URL and exit without firing it.
    [switch]$WhatIf
)

$ErrorActionPreference = 'Stop'

function Get-TargetFile([string]$AppName) {
    switch ($AppName) {
        'claude'   { Join-Path $env:USERPROFILE '.claude\settings.json' }
        'codex'    { Join-Path $env:USERPROFILE '.codex\config.toml' }
        'gemini'   { Join-Path $env:USERPROFILE '.gemini\.env' }
        'opencode' { Join-Path $env:USERPROFILE '.config\opencode\opencode.json' }
        'openclaw' { Join-Path $env:USERPROFILE '.openclaw\openclaw.json' }
    }
}

function Get-ProtocolRegistered {
    try {
        $null = Get-ItemProperty 'HKCU:\Software\Classes\aimyria' -ErrorAction Stop
        return $true
    } catch {
        return $false
    }
}

$target = Get-TargetFile $App
$url = "aimyria://v1/import?resource=provider&app=$App&name=Aimyria" +
       "&endpoint=https://api.aimyria.com&apiKey=sk-e2e-dummy-key&homepage=https://aimyria.com"

Write-Output "app    : $App"
Write-Output "target : $target"
Write-Output "url    : $url"
Write-Output ''

if (-not (Get-ProtocolRegistered)) {
    Write-Error @'
aimyria:// 未注册。

先启动一次 Aimyria（首次启动会注册协议），或确认安装程序已运行：
  & "$env:LOCALAPPDATA\Aimyria\AimyriaOne.exe"
'@
    exit 1
}
Write-Output 'protocol : registered'
Write-Output ''

if ($WhatIf) {
    Write-Output '-WhatIf：未触发。去掉这个开关即可实际写入。'
    exit 0
}

# Fingerprint the file so the change is visible rather than assumed. A config
# file is JSON, TOML or dotenv depending on the client, so compare raw bytes.
$before = if (Test-Path -LiteralPath $target) {
    (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash
} else {
    '<absent>'
}

Write-Output "before : $before"
Write-Output ''
Write-Output 'firing deep link…'
Start-Process "aimyria://v1/import?resource=provider&app=$App&name=Aimyria&endpoint=https://api.aimyria.com&apiKey=sk-e2e-dummy-key&homepage=https://aimyria.com"

# The app has to be running to receive the URL; give it a moment, then poll for
# the change rather than sleeping a fixed amount.
$deadline = (Get-Date).AddSeconds(20)
$after = $before
while ((Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 500
    $after = if (Test-Path -LiteralPath $target) {
        (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash
    } else {
        '<absent>'
    }
    if ($after -ne $before) { break }
}

Write-Output "after  : $after"
Write-Output ''

if ($after -eq $before) {
    Write-Error @"
$target 未发生变化。

可能原因：
  1. Aimyria 没在运行 —— 深链需要有接收方。先启动它再跑本脚本。
  2. 协议注册指向了另一个 Aimyria（开发构建 vs 安装版）——
     检查 HKCU:\Software\Classes\aimyria\shell\open\command 的值。
  3. 应用日志里有解析错误：$env:USERPROFILE\.aimyria\logs\aimyria-one.log
"@
    exit 1
}

Write-Output 'PASS：深链已改变客户端配置文件。'
Write-Output ''
Write-Output '请手工核对内容（脚本只比较哈希，不判断语义）：'
Write-Output "  $target"
Write-Output ''
Write-Output '注意：这次写入的是 dummy key。要恢复正常配置，重新从控制台获取一次。'