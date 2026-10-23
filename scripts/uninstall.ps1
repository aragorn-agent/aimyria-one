# AIMYRIA GO · 卸载
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
# 两代快捷方式名都要清：1.0.7 及以前叫「Aimyria 一键配置工具」，之后叫 AIMYRIA GO。
# 只清新名字会留下一个指向已删除 exe 的孤儿快捷方式 —— 用户点了会看到
# "找不到文件"，而卸载看上去是"成功了"。
$ShortcutPaths = @(
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\AIMYRIA GO.lnk'),
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Aimyria 一键配置工具.lnk')
)
$RunKey        = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
# 要清掉的 Run 值名。
# 注册表值名大小写不敏感，所以 'AIMYRIA GO' 与 'AimyriA go' 其实是**同一个值**；
# 两个都列上只是让意图自解释（重复删同一个值是无害的）。
# 'Aimyria One' 是真正不同的旧产品名，1.0.7 及以前写过。
$RunValueNames = @('AIMYRIA GO', 'AimyriA go', 'Aimyria One')
$UninstallKey  = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\AimyriaOne'
# Both schemes the app registers. Leaving `ccswitch` behind would keep
# `ccswitch://` links routed at a binary that no longer exists.
$ProtocolSchemes = @('aimyria', 'ccswitch')

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
if (Test-Path $RunKey) {
    foreach ($name in $RunValueNames) {
        $existing = (Get-ItemProperty -Path $RunKey -ErrorAction SilentlyContinue).$name
        if ($existing) {
            Say "移除开机自启：$name"
            Remove-ItemProperty -Path $RunKey -Name $name -Force -ErrorAction SilentlyContinue
        }
    }
}

if (Test-Path $UninstallKey) {
    Say '移除「设置 → 应用」中的卸载项'
    Remove-Item -Path $UninstallKey -Recurse -Force
}

foreach ($scheme in $ProtocolSchemes) {
    $key = "HKCU:\Software\Classes\$scheme"
    if (Test-Path $key) {
        Say "移除 ${scheme}:// 深链协议"
        Remove-Item -Path $key -Recurse -Force
    }
}

# ── 3. 快捷方式 ──────────────────────────────────────────────────────────
foreach ($sc in $ShortcutPaths) {
    if (Test-Path -LiteralPath $sc) {
        Say '移除开始菜单快捷方式'
        Remove-Item -LiteralPath $sc -Force
    }
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
} elseif (Test-Path -LiteralPath $userDir) {
    # 默认保留 ~/.aimyria（API Key、历史），但**删掉登录凭据**。
    #
    # 为什么区别对待：API Key 是用户自己可以revoke 的、有明确归属的配置；
    # 而 session.bin 是一个仍然可用的登录态。卸载后把一个还能登录账户的凭据
    # 留在磁盘上，没有任何理由——用户下次重装，重新登录一次的成本是 10 秒。
    #
    # 只覆盖一个文件名，而不是清空整个目录，所以 Key 和历史不受影响。
    $sessionFiles = @('session.bin', 'session.bin.tmp')
    foreach ($name in $sessionFiles) {
        $path = Join-Path $userDir $name
        if (Test-Path -LiteralPath $path) {
            # 先覆写再删：机械盘上被 unlink 的内容仍可能残留在空闲块里。
            try {
                $len = [Math]::Min((Get-Item -LiteralPath $path).Length, 65536)
                if ($len -gt 0) {
                    [System.IO.File]::WriteAllBytes($path, (New-Object byte[] $len))
                }
            } catch { }
            Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
            Say "已清除登录凭据（API Key 与历史保留）"
        }
    }
}

if (-not $Uninstall) {
    Write-Output ''
    Write-Output '  AIMYRIA GO 已卸载。'
    if (-not $RemoveUserData) {
        Write-Output "  账号与配置仍保留在 $userDir，重新安装即可继续使用。"
    }
}