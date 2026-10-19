# Give Windows enough pagefile to compile a large Rust binary.
#
# # What is wrong right now
#
# The machine has 16 GB of RAM and a pagefile that is pinned at a 2 GB quota with
# system-managed sizing switched off — while 14.8 GB of it was already in use during
# a build. That is not a margin, it is a coin flip: `rustc` died with
# "rustc-LLVM ERROR: out of memory / Allocation failed" partway through linking the
# binary, and the only thing that got the build through was `-j 1`.
#
# # Why system-managed rather than a fixed number
#
# A hard-coded maximum is a guess that is wrong on a different machine. Letting
# Windows size it means the next project does not hit this either. The setting that
# matters is AutomaticManagedPagefile; the explicit size below only covers the case
# where that cannot be set.
#
# # About the reboot
#
# Switching pagefile management on normally takes effect after a restart. The
# script does NOT restart anything — a reboot mid-build destroys the work. It
# reports what was set and when it will take effect, and leaves that call alone.
#
# Run as administrator. Re-launches itself elevated and logs to a file, since an
# elevated process cannot write back to the one that started it.

param(
  [string]$Log = "D:\src\cc-switch-main\.pagefile.log",
  [int]$InitialGB = 8,
  [int]$MaxGB     = 32
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
  Say "not administrator - relaunching elevated (accept the UAC prompt)"
  try {
    Start-Process -FilePath "powershell.exe" -Verb RunAs -Wait -ArgumentList @(
      "-NoProfile", "-ExecutionPolicy", "Bypass",
      "-File", ('"{0}"' -f $PSCommandPath),
      "-Log", ('"{0}"' -f $Log)
    )
  } catch {
    Say "elevation failed: $($_.Exception.Message)"
    exit 1
  }
  Get-Content -LiteralPath $Log | ForEach-Object { Write-Output $_ }
  exit 0
}

Say "running elevated"

# ── before ───────────────────────────────────────────────────────────────────
$cs = Get-CimInstance Win32_ComputerSystem
Say ("before: AutomaticManagedPagefile = {0}" -f $cs.AutomaticManagedPagefile)
Get-CimInstance Win32_PageFileSetting -EA SilentlyContinue | ForEach-Object {
  Say ("        {0}  initial {1} MB  max {2} MB" -f $_.Name, $_.InitialSize, $_.MaximumSize)
}
$usage = Get-CimInstance Win32_PageFileUsage -EA SilentlyContinue
if ($usage) { Say ("        peak usage {0} MB" -f (($usage | Measure-Object PeakUsage -Maximum).Maximum)) }

# ── change ───────────────────────────────────────────────────────────────────
$ok = $true
try {
  Set-CimInstance -InputObject $cs -Property @{ AutomaticManagedPagefile = $true } -EA Stop
  Say "set AutomaticManagedPagefile = $true"
} catch {
  Say "could not switch to system-managed: $($_.Exception.Message)"
  $ok = $false
}

if (-not $ok) {
  # Fall back to an explicit floor. Removing the old setting first is required;
  # Windows will not resize a pagefile in place.
  try {
    Get-CimInstance Win32_PageFileSetting -EA SilentlyContinue | ForEach-Object {
      Remove-CimInstance -InputObject $_ -EA Stop
      Say "removed existing pagefile setting for $($_.Name)"
    }
    New-CimInstance -ClassName Win32_PageFileSetting -Property @{
      Name         = "C:\pagefile.sys"
      InitialSize  = $InitialGB * 1024
      MaximumSize  = $MaxGB * 1024
    } -EA Stop | Out-Null
    Say "set explicit pagefile ${InitialGB}G .. ${MaxGB}G on C:"
  } catch {
    Say "explicit fallback also failed: $($_.Exception.Message)"
    exit 1
  }
}

# ── after ────────────────────────────────────────────────────────────────────
$cs2 = Get-CimInstance Win32_ComputerSystem
Say ("after:  AutomaticManagedPagefile = {0}" -f $cs2.AutomaticManagedPagefile)
Get-CimInstance Win32_PageFileSetting -EA SilentlyContinue | ForEach-Object {
  Say ("        {0}  initial {1} MB  max {2} MB" -f $_.Name, $_.InitialSize, $_.MaximumSize)
}
Say "a reboot is needed for the new size to be committed; nothing was restarted here"
Say "RESULT=OK"
