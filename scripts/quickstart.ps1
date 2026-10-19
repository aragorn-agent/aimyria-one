# Aimyria One · 静默安装
#
# 用法：
#   irm https://aimyria.com/quickstart.ps1 | iex
#
# 做四件事：下载、校验、装到当前用户目录、启动。
#
# 为什么不打包成 NSIS 安装器：
#   我们不做代码签名（见 README）。一个需要点「下一步 / 我同意 / 安装」的
#   未签名安装器，对用户来说比一个直接放进 %LOCALAPPDATA% 的 exe 更可疑——
#   它看起来像在安装别的东西，而不是在运行一个可自己构建的程序。
#   整个产品已经是一个托盘常驻工具，装成「绿色版」反而更符合它的形态。
#
# 副作用清单（全部可逆）：
#   - 新建 %LOCALAPPDATA%\Aimyria\
#   - 新建开始菜单项 "Aimyria 一键配置工具"
#   - 新建 HKCU\...\CurrentVersion\Run 项（开机自启，装完可在设置里关）
#   - 首次启动时注册 HKCU\Software\Classes\aimyria 深链协议
#
# 不需要管理员权限，不写 Program Files，不改系统 PATH。

#requires -Version 5.1
[CmdletBinding()]
param(
    # 覆盖下载地址，便于自建镜像与内网部署。
    [string]$BaseUrl = 'https://aimyria.com',
    # 跳过 SHA256 校验。除非你清楚后果，否则不要用。
    [switch]$SkipChecksum,
    # 只下载并打印安装路径，不落盘。
    [switch]$WhatIfOnly
)

$ErrorActionPreference = 'Stop'

function Write-Step([string]$Message) {
    # 静默模式下 Write-Host 走 stdout，可能被管道吞掉，所以统一写 stderr。
    [Console]::Error.WriteLine("  $Message")
}
function Write-Warn2([string]$Message) {
    [Console]::Error.WriteLine("! $Message")
}
function Die([string]$Message) {
    [Console]::Error.WriteLine("x $Message")
    exit 1
}

Write-Step 'Aimyria 一键配置工具 · 安装'

# ── 1. 环境 ──────────────────────────────────────────────────────────────
if (-not [System.Environment]::Is64BitOperatingSystem) {
    Die '需要 64 位 Windows。'
}

# TLS 1.2：老系统的默认协议集里没有它，下载会失败在一个很难懂的地方。
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
} catch {
    Write-Warn2 '无法设置 TLS 1.2，下载可能失败。'
}

# ── 2. 路径 ──────────────────────────────────────────────────────────────
$InstallDir   = Join-Path $env:LOCALAPPDATA 'Aimyria'
$ExePath      = Join-Path $InstallDir 'AimyriaOne.exe'
$LoaderDll    = Join-Path $InstallDir 'WebView2Loader.dll'
$StartMenuDir = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$ShortcutPath = Join-Path $StartMenuDir 'Aimyria 一键配置工具.lnk'
$RunKey       = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$RunValueName = 'Aimyria One'

$DownloadUrl  = "$BaseUrl/releases/latest/download/AimyriaOne-x64.exe"
$ChecksumUrl  = "$BaseUrl/releases/latest/download/AimyriaOne-x64.exe.sha256"
$LoaderUrl    = "$BaseUrl/releases/latest/download/WebView2Loader.dll"
$LoaderShaUrl = "$BaseUrl/releases/latest/download/WebView2Loader.dll.sha256"

# 卸载脚本随安装器一起分发，放进安装目录，这样「设置 → 应用」里的卸载项
# 永远指向一个存在的文件——而不是 aimyria.com 改版后 404。
$UninstallScript = Join-Path $InstallDir 'uninstall.ps1'

# ── 3. 下载到临时目录 ────────────────────────────────────────────────────
# 先下完、校验完，再动安装目录：中途失败时用户机器上不会留下半个安装。
$stage = Join-Path $env:TEMP ("Aimyria-stage-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $stage | Out-Null

function Get-Verified([string]$Url, [string]$OutFile, [string]$Label) {
    Write-Step "正在下载 $Label …"
    try {
        # UseBasicParsing：PS 5.1 不写它会因为 IE 引擎被禁用而报错，
        # 而目标机器上根本没装 IE。
        Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing
    } catch {
        Die "下载失败：$($_.Exception.Message)`n请手动打开 $BaseUrl/download 下载后运行。"
    }

    if ($SkipChecksum) {
        Write-Warn2 "$Label 跳过完整性校验。"
        return
    }

    try {
        $raw = (Invoke-WebRequest -Uri "$Url.sha256" -UseBasicParsing).Content
        # `.Content` comes back as byte[] whenever the server did not send a text
        # content type, and byte[] has no Trim(). Decode first rather than
        # assuming a well-behaved server.
        if ($raw -is [byte[]]) { $raw = [System.Text.Encoding]::UTF8.GetString($raw) }
        $Expected = ([string]$raw).Trim().Split()[0].ToLower()
        $Actual = (Get-FileHash -LiteralPath $OutFile -Algorithm SHA256).Hash.ToLower()
        if ($Expected -ne $Actual) {
            Die "$Label 校验和不匹配，文件可能已损坏或被篡改。`n  期望 $Expected`n  实际 $Actual"
        }
        Write-Step "$Label 校验通过。"
    } catch {
        # 拿不到校验和不该直接失败——镜像可能没同步这个文件——但必须把原因
        # 说出来，否则用户看到的是「校验通过」的假象。
        Write-Warn2 "$Label 未能校验（$($_.Exception.Message)）。安装继续，但没有验证过来源。"
    }
}

$stagedExe = Join-Path $stage 'AimyriaOne.exe'
Get-Verified $DownloadUrl $stagedExe 'Aimyria One'

# exe 依赖的 WebView2 加载器。缺了它，程序在部分 Windows 上起不来
# （STATUS_ENTRYPOINT_NOT_FOUND），而且症状是"双击没反应"，极难自查。
$stagedDll = Join-Path $stage 'WebView2Loader.dll'
if (-not $SkipChecksum) {
    try {
        Get-Verified $LoaderUrl $stagedDll 'WebView2Loader.dll'
    } catch {
        Write-Warn2 '加载器下载失败，将只安装 exe。'
    }
}

Write-Step ''
Write-Step ("  " + $stagedExe)
Write-Step ("  " + [Math]::Round((Get-Item $stagedExe).Length / 1MB, 1) + " MB")
Write-Step ''

if ($WhatIfOnly) {
    Write-Step "-WhatIfOnly：未安装。安装目录将是 $InstallDir"
    Remove-Item -LiteralPath $stage -Recurse -Force
    exit 0
}

# ── 4. 落盘 ──────────────────────────────────────────────────────────────
if (Test-Path -LiteralPath $ExePath) {
    Write-Step '检测到已有安装，将覆盖更新。'
}

try {
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
} catch {
    Die "无法创建安装目录 $InstallDir ：$($_.Exception.Message)"
}

# 复制到同目录的临时名再改名：程序可能正在运行，直接覆盖会失败或留下半个 exe。
$tmpExe = Join-Path $InstallDir 'AimyriaOne.exe.new'
try {
    Copy-Item -LiteralPath $stagedExe -Destination $tmpExe -Force
    if (Test-Path -LiteralPath $ExePath) {
        # 正在运行的话这一步会失败——先告诉用户，而不是静默跳过。
        Remove-Item -LiteralPath $ExePath -Force -ErrorAction Stop
    }
    Move-Item -LiteralPath $tmpExe -Destination $ExePath -Force
} catch {
    Remove-Item -LiteralPath $tmpExe -Force -EA SilentlyContinue
    Die "写入失败：$($_.Exception.Message)`n如果 Aimyria 正在运行，请先从托盘退出。"
}

if (Test-Path -LiteralPath $stagedDll) {
    Copy-Item -LiteralPath $stagedDll -Destination $LoaderDll -Force -EA SilentlyContinue
}

# 卸载脚本随包分发：卸载入口指向本地文件，不依赖网站永远可用。
$uninstallSource = Join-Path $PSScriptRoot 'uninstall.ps1'
if (Test-Path -LiteralPath $uninstallSource) {
    Copy-Item -LiteralPath $uninstallSource -Destination $UninstallScript -Force
}

Remove-Item -LiteralPath $stage -Recurse -Force -EA SilentlyContinue

# ── 5. 开始菜单快捷方式 ──────────────────────────────────────────────────
try {
    $shell = New-Object -ComObject WScript.Shell
    $sc = $shell.CreateShortcut($ShortcutPath)
    $sc.TargetPath = $ExePath
    $sc.WorkingDirectory = $InstallDir
    $sc.Description = '把 AI 编程命令行一键指向 Aimyria'
    $sc.Save()
} catch {
    Write-Warn2 '无法创建开始菜单快捷方式，可直接从安装目录运行。'
}

# ── 6. 卸载入口 ──────────────────────────────────────────────────────────
# NSIS 安装器通常会写这一项；我们直装，所以自己写。不写的话
# 「设置 → 应用」里看不到这个程序，用户只能用文件管理器删——
# 那会留下开始菜单快捷方式和开机自启项，是不完整的卸载。
try {
    $uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\AimyriaOne'
    New-Item -Path $uninstallKey -Force -ErrorAction Stop | Out-Null
    # Set-ItemProperty takes -Name; -PropertyName belongs to New-ItemProperty.
    Set-ItemProperty -Path $uninstallKey -Name 'DisplayName'      -Value 'Aimyria 一键配置工具' -ErrorAction Stop
    Set-ItemProperty -Path $uninstallKey -Name 'DisplayVersion'   -Value '1.0.0'   -ErrorAction Stop
    Set-ItemProperty -Path $uninstallKey -Name 'Publisher'        -Value 'Aimyria'  -ErrorAction Stop
    Set-ItemProperty -Path $uninstallKey -Name 'DisplayIcon'      -Value "$ExePath,0" -ErrorAction Stop
    Set-ItemProperty -Path $uninstallKey -Name 'InstallLocation'  -Value $InstallDir -ErrorAction Stop
    Set-ItemProperty -Path $uninstallKey -Name 'NoModify'         -Value 1 -Type DWord -ErrorAction Stop
    Set-ItemProperty -Path $uninstallKey -Name 'NoRepair'         -Value 1 -Type DWord -ErrorAction Stop
    # 静默卸载：设置 → 应用里的「卸载」不会弹任何界面
    Set-ItemProperty -Path $uninstallKey -Name 'QuietUninstallString' -ErrorAction Stop `
        -Value ('powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "{0}" -Uninstall' -f $UninstallScript)
} catch {
    Write-Warn2 "无法写入卸载信息（$($_.Exception.Message)）。可手动运行 $UninstallScript 卸载。"
}

# ── 7. 启动 ──────────────────────────────────────────────────────────────
# 开机自启由程序首次运行时自己注册（它知道自己的最终路径），
# 这里不重复写——两处写同一个 Run 项必然会漂移。
Write-Step '安装完成，正在启动…'
try {
    Start-Process -FilePath $ExePath | Out-Null
} catch {
    Write-Warn2 "启动失败，可手动运行 $ExePath"
}

Write-Step ''
Write-Step "安装目录：$InstallDir"
Write-Step '下一步：打开 https://aimyria.com/console 点「一键配置」。'
Write-Step '工具常驻系统托盘，不显示窗口；需要时可从托盘图标打开。'
Write-Step ''
Write-Step '卸载：设置 → 应用 → Aimyria 一键配置工具。'