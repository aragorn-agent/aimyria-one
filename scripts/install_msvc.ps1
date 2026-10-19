# Install Visual Studio Build Tools so the MSVC toolchain can link.
#
# # Why this is needed
#
# The release is built with `x86_64-pc-windows-gnu`, and on that target the WebView2
# loader can only be linked dynamically. That puts `WebView2Loader.dll` in the exe's
# ordinary import table — loaded by the OS before `main()` runs — so the binary only
# starts if someone has put that DLL in the same folder. Copying the exe on its own
# to another machine fails at launch with "无法找到 WebView2Loader.dll".
#
# On `x86_64-pc-windows-msvc` the WebView2 SDK's `WebView2LoaderStatic.lib` links into
# the binary, so the result is a single file that runs anywhere. The same switch also
# makes `build.rs`'s Common Controls v6 workaround apply — it is guarded on
# `target_env = "msvc"`, which is why the Rust unit tests currently die with
# STATUS_ENTRYPOINT_NOT_FOUND (0xC0000139).
#
# # Why this script elevates itself
#
# The first attempt failed with exit code 1602, "user cancelled installation", after
# two minutes and with nothing on disk — no `D:\VSBuildTools`, not even a `dd_*.log`.
# That is what the Visual Studio bootstrapper does when it is launched without
# administrator rights: it stops before it starts logging, so the log tells you
# nothing and 1602 reads like a cancellation that never happened.
#
# The installer writes to stdout, which an elevated process cannot hand back to this
# one, so everything is mirrored to a log file instead and that file is what gets
# read afterwards.
#
# # Why D:
#
# C: has ~26 GB free and the C++ workload wants 7–8 GB. Keeping the install off the
# system drive means a full C: is never the reason a build fails later.

param(
  [string]$InstallPath = "D:\VSBuildTools",
  [string]$Sdk         = "Microsoft.VisualStudio.Component.Windows11SDK.22621",
  [string]$Log         = "D:\src\cc-switch-main\.msvc-install.log"
)

$ErrorActionPreference = "Continue"

function Say($m) {
  $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m
  Write-Output $line
  Add-Content -LiteralPath $Log -Value $line -Encoding utf8
}

$isAdmin = ([Security.Principal.WindowsPrincipal](
    [Security.Principal.WindowsIdentity]::GetCurrent()
  )).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

if (-not $isAdmin) {
  Set-Content -LiteralPath $Log -Value "" -Encoding utf8
  Say "not administrator — relaunching elevated (accept the UAC prompt)"
  try {
    Start-Process -FilePath "powershell.exe" -Verb RunAs -Wait -ArgumentList @(
      "-NoProfile", "-ExecutionPolicy", "Bypass",
      "-File", ('"{0}"' -f $PSCommandPath),
      "-InstallPath", ('"{0}"' -f $InstallPath),
      "-Sdk", ('"{0}"' -f $Sdk),
      "-Log", ('"{0}"' -f $Log)
    )
  } catch {
    Say "elevation refused or failed: $($_.Exception.Message)"
    Say "RESULT=FAIL"
    exit 1
  }
  Get-Content -LiteralPath $Log | ForEach-Object { Write-Output $_ }
  exit 0
}

Say "running elevated as $env:USERNAME"
$bt = Join-Path $env:TEMP "vs_BuildTools.exe"

if (-not (Test-Path -LiteralPath $bt) -or (Get-Item $bt).Length -lt 1MB) {
  Say "downloading the bootstrapper (~4 MB)"
  $ProgressPreference = "SilentlyContinue"
  Invoke-WebRequest -Uri "https://aka.ms/vs/17/release/vs_BuildTools.exe" `
                    -OutFile $bt -UseBasicParsing -TimeoutSec 600
}
Say ("bootstrapper {0:N2} MB" -f ((Get-Item $bt).Length / 1MB))

$a = @(
  "--quiet", "--wait", "--norestart", "--nocache",
  "--installPath", $InstallPath,
  "--add", "Microsoft.VisualStudio.Workload.VCTools",
  "--add", $Sdk,
  "--includeRecommended"
)
Say ("invoking: " + ($a -join " "))

$sw = [Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath $bt -ArgumentList $a -PassThru -Wait
$code = $p.ExitCode
$sw.Stop()
Say ("installer exit {0} after {1:N1} min" -f $code, $sw.Elapsed.TotalMinutes)

# 0 = success, 3010 = success with a pending reboot. Either way the toolchain is on
# disk; a pending reboot only affects whether a fresh shell sees the SDK environment.
if ($code -ne 0 -and $code -ne 3010) {
  Say "RESULT=FAIL exit=$code"
  Get-ChildItem "$env:TEMP" -Filter "dd_*.log" -File -EA SilentlyContinue |
    Sort-Object LastWriteTime -Descending | Select-Object -First 2 |
    ForEach-Object {
      Say ("--- " + $_.Name + " ---")
      Get-Content $_.FullName -Tail 20 | ForEach-Object { Add-Content -LiteralPath $Log -Value ("    " + $_) -Encoding utf8 }
    }
  exit 1
}

# The install reporting success is not the same claim as a link being possible, so
# check the two artefacts that decide it.
$link = Get-ChildItem $InstallPath -Recurse -Include link.exe -File -EA SilentlyContinue | Select-Object -First 1
Say ("link.exe: " + $(if ($link) { $link.FullName } else { "NOT FOUND" }))

$libDir = "C:\Program Files (x86)\Windows Kits\10\Lib"
$sdks = if (Test-Path $libDir) { (Get-ChildItem $libDir -Directory | Select-Object -ExpandProperty Name) -join ", " } else { "MISSING" }
Say ("Windows SDK Lib: $sdks")

if (-not $link) { Say "RESULT=FAIL no-link"; exit 1 }
Say "RESULT=OK"
exit 0
