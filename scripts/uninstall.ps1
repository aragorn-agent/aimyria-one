# Aimyria One · 卸载
#
#   powershell -File uninstall.ps1
#   powershell -File uninstall.ps1 -Uninstall      # 静默模式（设置→应用 里调的就是这个）
#   powershell -File uninstall.ps1 -KeepConfig     # 保留 ~/.aimyria 里的 Key 与历史
#
# 删干净：程序文件、开始菜单快捷方式、开机自启项、深链协议注册、卸载项本身。
#
# 默认保留 ~/.aimyria：里面是用户的 API Key 和已配置的客户端。
# 删掉它等于让用户重新绑定账号，而他们卸载这个工具的原因往往只是
# 「暂时不用」，不是「不想要这个账号」。要给 Key 再跑一次
# quickstart.ps1 -RemoveUserData。

[CmdletBinding()]
param(
    [switch]$Uninstall,
    # 连同 %LOCALAPPDATA%\Aimyria 一起删（API Key、数据库、日志）。
    [switch]$RemoveUserData
)

$ErrorActionPreference = 'SilentlyContinue'

function Say([string]$m) {
    if (-not $Uninstall) { Write-Output "  $m" }
}

$InstallDir    = Join-Path $env:LOCALAPPDATA 'Aimyria'
$ShortcutPath  = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Aimyria 一键配置工具.lnk'
$RunKey        = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$RunValueName  = 'Aimyria One'
$UninstallKey  = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\AimyriaOne'
$ProtocolKey   = 'HKCU:\Software\Classes\aimyria'

# ── 1. 停掉正在运行的实例 ────────────────────────────────────────────────
# 不停就删不掉 exe，而且用户会以为卸载失败。
$running = Get-Process -Name 'AimyriaOne' -EA SilentlyContinue
if ($running) {
    Say '正在关闭运行中的 Aimyria…'
    $running | Stop-Process -Force
    # 给它一点时间真正退出，否则紧接着的 Remove-Item 会被占用挡住。
    $running | Wait-Process -Timeout 10
}

# ── 2. 注册表 ────────────────────────────────────────────────────────────
foreach ($item in @(
    @{ Path = $RunKey;       Name = $RunValueName },
    @{ Path = $UninstallKey; Name = $null }
)) {
    if (-not (Test-Path $item.Path)) { continue }
    if ($item.Name) {
        $existing = (Get-ItemProperty -Path $item.Path).$($item.Name)
        if ($existing) {
            Say "移除开机自启：$($item.Name)"
            Remove-ItemProperty -Path $item.Path -Name $item.Name -Force
        }
    } else {
        Say '移除「设置 → 应用」中的卸载项'
        Remove-Item -Path $item.Path -Recurse -Force
    }
}

if (Test-Path $ProtocolKey) {
    Say '移除 aimyria:// 深链协议'
    Remove-Item -Path $ProtocolKey -Recurse -Force
}

# ── 3. 快捷方式 ──────────────────────────────────────────────────────────
if (Test-Path -LiteralPath $ShortcutPath) {
    Say '移除开始菜单快捷方式'
    Remove-Item -LiteralPath $ShortcutPath -Force
}

# ── 4. 程序文件 ──────────────────────────────────────────────────────────
if (Test-Path -LiteralPath $InstallDir) {
    if ($RemoveUserData) {
        Say "删除 $InstallDir（含 API Key、数据库、日志）"
        Remove-Item -LiteralPath $InstallDir -Recurse -Force
    } else {
        # uninstall.ps1 自己在 $InstallDir 里，先把它挪出来再删目录。
        $self = $MyInvocation.MyCommand.Path
        $me = Join-Path $env:TEMP 'aimyria-uninstall-running.ps1'
        Copy-Item -LiteralPath $self -Destination $me -Force
        Say "删除 $InstallDir（保留 ~/.aimyria）"
        Remove-Item -LiteralPath $InstallDir -Recurse -Force
        Remove-Item -LiteralPath $me -Force
    }
}

# ── 5. 用户数据 ──────────────────────────────────────────────────────────
$userDir = Join-Path $env:USERPROFILE '.aimyria'
if ($RemoveUserData -and (Test-Path -LiteralPath $userDir)) {
    Say "删除 $userDir"
    Remove-Item -LiteralPath $userDir -Recurse -Force
}

if (-not $Uninstall) {
    Write-Output ''
    Write-Output '  Aimyria One 已卸载。'
    if (-not $RemoveUserData) {
        Write-Output "  账号与配置仍保留在 $userDir，重新安装即可继续使用。"
    }
}