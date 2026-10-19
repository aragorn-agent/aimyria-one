# Cold-start deep link test: does the confirmation dialog actually appear?
#
# The product promise is "click import in the browser, confirm once, done". The
# link is received in Rust and forwarded to the renderer as a `deeplink-import`
# event; the dialog only exists if React was already mounted and listening when
# that event was emitted. A cold start is the case where it plausibly is not:
# the process is launched *by* the link, so the emit races the renderer's
# `listen()` registration.
#
# # Why a screenshot is the deciding evidence
#
# The dialog is a Radix Dialog inside the webview, so it has no HWND of its own
# and window enumeration cannot see it. A log line cannot decide it either: an
# event with no listener is dropped silently by design, so "nothing in the log"
# is consistent with both success and failure. Only the rendered pixels separate
# "window up, dialog showing" from "window up, blank".
#
# # Why the window is captured, not the screen
#
# A full-screen grab would also capture whatever the session has in front. Using
# PrintWindow against the app's HWND captures what that window is drawing, which
# is the claim under test.

param(
  [string]$Exe = (Join-Path (Split-Path -Parent $PSScriptRoot) "release\AimyriaOne-x64.exe"),
  [string]$Url = "aimyria://v1/import?resource=provider&app=claude&name=Aimyria&endpoint=https://api.aimyria.com&apiKey=sk-placeholder-000000000000",
  [string]$OutDir = (Join-Path (Split-Path -Parent $PSScriptRoot) ".visual-check"),
  [int]$SettleSeconds = 18
)

$ErrorActionPreference = "Continue"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

if (-not ("WinCap" -as [type])) {
  Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Text;
public static class WinCap {
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern int GetWindowTextLength(IntPtr h);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    // Deliberately no SetWindowPos / ShowWindow / SetForegroundWindow here. The
    // window under test carries a live Import button, so raising it or stealing
    // focus is how a verification run can cause a real write. P/Invokes that are
    // not declared cannot be called by accident later either.
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    public static string ClassOf(IntPtr h) {
        var sb = new StringBuilder(256); GetClassName(h, sb, 256); return sb.ToString();
    }

    public static List<IntPtr> VisibleWindows(uint target) {
        var found = new List<IntPtr>();
        EnumWindows((h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (pid == target && IsWindowVisible(h)) found.Add(h);
            return true;
        }, IntPtr.Zero);
        return found;
    }

    // The app's real UI window. Enumerating "visible windows" is not enough:
    // this process also owns a visible 16x16 `com.aimyria.desktop-sic` helper
    // and a `Tao Thread Event Target`, so an unfiltered count is never the
    // window under test.
    public static List<IntPtr> VisibleMainWindows(uint target) {
        var found = new List<IntPtr>();
        EnumWindows((h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (pid == target && IsWindowVisible(h) && ClassOf(h) == "Tauri Window") found.Add(h);
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static string Describe(IntPtr h) {
        RECT r; GetWindowRect(h, out r);
        return "hwnd=0x" + h.ToString("X") + " class='" + ClassOf(h) + "' rect=" + r.L + "," + r.T + "," + r.R + "," + r.B
             + " size=" + (r.R - r.L) + "x" + (r.B - r.T) + " titleLen=" + GetWindowTextLength(h);
    }

    // Captures the window's own screen region WITHOUT raising it.
    //
    // An earlier version of this script called SetWindowPos(TOPMOST) and
    // SetForegroundWindow first. That was both unnecessary and unsafe: the
    // window under test holds a live Import button, so forcing it to the front
    // of a desktop that other people are also using invites a real click on a
    // button that writes a provider into the app's database. It happened once
    // during this task. Reading the window's own rect changes nothing about
    // z-order or focus, so the capture is not a reason for anyone to click.
    //
    // The tradeoff is that another window may overlap the region. The overlap is
    // visible in the frame, and PrintWindow is tried as a fallback, so a bad
    // capture is reported as unusable rather than silently believed.
    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);

    public static bool CapturePrintWindow(IntPtr h, string path) {
        RECT r; if (!GetWindowRect(h, out r)) return false;
        int w = r.R - r.L, ht = r.B - r.T;
        if (w <= 0 || ht <= 0) return false;
        using (var bmp = new Bitmap(w, ht, PixelFormat.Format32bppArgb))
        using (var g = Graphics.FromImage(bmp)) {
            IntPtr hdc = g.GetHdc();
            bool ok = PrintWindow(h, hdc, 2);
            g.ReleaseHdc(hdc);
            if (!ok) return false;
            bmp.Save(path, ImageFormat.Png);
        }
        return true;
    }

    // Reads pixels already on screen; no z-order or focus change.
    public static bool CaptureScreen(IntPtr h, string path, int settleMs) {
        RECT r; if (!GetWindowRect(h, out r)) return false;
        int w = r.R - r.L, ht = r.B - r.T;
        if (w <= 0 || ht <= 0) return false;
        if (settleMs > 0) System.Threading.Thread.Sleep(settleMs);
        using (var bmp = new Bitmap(w, ht, PixelFormat.Format32bppArgb))
        using (var g = Graphics.FromImage(bmp)) {
            g.CopyFromScreen(r.L, r.T, 0, 0, new Size(w, ht), CopyPixelOperation.SourceCopy);
            bmp.Save(path, ImageFormat.Png);
        }
        return true;
    }

    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT p);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }

    // How much of the window's area actually belongs to this process.
    //
    // This is the check the CopyFromScreen path needs and that a colour test
    // cannot provide. A screen capture reads whatever is on top, so if another
    // window covers the app, the pixels are that other window's -- and they are
    // richly coloured, so a "has content" test reports USABLE. That happened
    // here: an editor was overlapping the app and the capture was reported
    // usable while containing none of the app. WindowFromPoint returns the
    // topmost window at a point, so sampling a grid over the rect says plainly
    // whether the region is ours. `own` well below `total` means the capture is
    // not of this window and must not be used as evidence.
    public static string Coverage(IntPtr h, uint target) {
        RECT r; if (!GetWindowRect(h, out r)) return "n/a";
        int w = r.R - r.L, ht = r.B - r.T;
        if (w <= 0 || ht <= 0) return "n/a";
        int own = 0, total = 0;
        for (int yi = 1; yi <= 9; yi++)
          for (int xi = 1; xi <= 9; xi++) {
            int x = r.L + w * xi / 10, y = r.T + ht * yi / 10;
            IntPtr top = WindowFromPoint(new POINT { X = x, Y = y });
            uint pid; GetWindowThreadProcessId(top, out pid);
            total++;
            if (pid == target) own++;
          }
        return "own=" + own + "/" + total;
    }
    public static int CoverageOwn(string s) {
        var m = System.Text.RegularExpressions.Regex.Match(s, @"own=(\d+)/");
        return m.Success ? int.Parse(m.Groups[1].Value) : -1;
    }
    public static int CoverageTotal(string s) {
        var m = System.Text.RegularExpressions.Regex.Match(s, @"/(\d+)$");
        return m.Success ? int.Parse(m.Groups[1].Value) : -1;
    }

    // A capture is only evidence if it has content.
    //
    // PrintWindow returns a solid black frame for this app's WebView2 window on
    // a hidden/occluded surface, and a flat frame proves nothing about whether a
    // dialog is on screen. `AllOneColour` is what turns "the capture succeeded"
    // into "the capture is usable"; a flat frame is reported as INCONCLUSIVE
    // rather than as a pass.
    public static string Stats(string path) {
        using (var bmp = new Bitmap(path)) {
            long sum = 0; int min = 255, max = 0; var distinct = new HashSet<int>();
            for (int y = 0; y < bmp.Height; y += 4)
              for (int x = 0; x < bmp.Width; x += 4) {
                var c = bmp.GetPixel(x, y); int v = (c.R + c.G + c.B) / 3;
                sum += v; if (v < min) min = v; if (v > max) max = v;
                if (distinct.Count < 4096) distinct.Add(c.R << 16 | c.G << 8 | c.B);
            }
            int n = (bmp.Width / 4 + 1) * (bmp.Height / 4 + 1);
            return "avg=" + (sum / Math.Max(n,1)) + " min=" + min + " max=" + max + " distinctColors~" + distinct.Count;
        }
    }
}
'@ -ReferencedAssemblies System.Drawing -ErrorAction SilentlyContinue
}

# Every instance, found by executable path rather than by process name.
#
# `Get-Process -Name aimyria-one` was wrong here, and silently so: the process
# name is derived from the *file name*, so the delivered `AimyriaOne-x64.exe`
# runs as `AimyriaOne-x64`, a temp copy as `AimyriaOne`, and only the cargo
# artifact as `aimyria-one`. Filtering on the cargo name therefore reported
# "0 instances" while an instance was running -- the cold-state gate passed
# when it should have failed, the launch was absorbed by the single-instance
# plugin, and the run produced a confident but meaningless "window never
# showed". Matching on the path substring is independent of what the file is
# called, which is the property that makes it correct as a gate.
function Get-AppProcesses {
  @(Get-Process -ErrorAction SilentlyContinue |
      Where-Object { $_.Path -and ($_.Path -match 'aimyria' -or $_.Path -match 'Aimyria') })
}
function Get-AppPids { @(Get-AppProcesses | Select-Object -ExpandProperty Id) }

Write-Output ("exe  : " + $Exe)
Write-Output ("url  : " + ($Url -replace 'apiKey=[^&]+', 'apiKey=<redacted>'))
Write-Output ("out  : " + $OutDir)
Write-Output ""

# -- 1. ensure nothing is running (cold start is the whole point) ------------
Write-Output "=== 1. cold state ==="
$before = Get-AppPids
Write-Output ("  aimyria-one pids before: " + $(if ($before) { $before -join ", " } else { "(none)" }))
if ($before) {
  $before | ForEach-Object { Stop-Process -Id $_ -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Seconds 3
}
$preexisting = Get-AppPids
Write-Output ("  pids after kill: " + $(if ($preexisting) { $preexisting -join ", " } else { "(none)" }))
if ($preexisting) {
  Write-Output "  RESULT=INVALID - could not reach a cold state; a later launch would just be absorbed by the running instance"
  exit 3
}
Write-Output "  [PASS] cold state confirmed: 0 instances, so the launch below is a genuine cold start"

# -- 2. launch with the URL as argv (this is what a protocol cold start is) --
Write-Output ""
Write-Output "=== 2. cold launch with the deep link as a command-line argument ==="
# Launching the exe directly with the URL as argv[1] reproduces exactly what
# Windows does when a protocol handler cold-starts the process: the shell looks
# up aimyria:// -> the registered command -> `"<exe>" "%1"`. It also works for a
# copy in a temp folder, which a registry-based Start-Process would not reach.
Write-Output ("  " + $Exe + " `"" + ($Url -replace 'apiKey=[^&]+', 'apiKey=<redacted>') + "`"")
$proc = Start-Process -FilePath $Exe -ArgumentList @($Url) -PassThru
Write-Output ("  launched pid " + $proc.Id + " at " + (Get-Date -Format "HH:mm:ss"))
$pids = @($proc.Id)

# -- 3. watch the window appear, then capture -------------------------------
Write-Output ""
Write-Output "=== 3. window appearance over time (class 'Tauri Window' only) ==="
$appeared = $false
for ($i = 1; $i -le 10; $i++) {
  $vis = @()
  foreach ($id in $pids) { $vis += [WinCap]::VisibleMainWindows([uint32]$id) }
  $at = Get-Date -Format "HH:mm:ss"
  Write-Output ("  t=" + ($i * 2) + "s (" + $at + ") visible 'Tauri Window': " + $vis.Count)
  foreach ($h in $vis) { Write-Output ("      " + [WinCap]::Describe($h)) }
  if ($vis.Count -gt 0 -and -not $appeared) { $appeared = $true; Write-Output "  -> main window became visible at t=$($i*2)s" }
  Start-Sleep -Seconds 2
}
if (-not $appeared) {
  Write-Output ""
  Write-Output "  RESULT=WINDOW_NEVER_SHOWED - the deep link did not raise the window"
  Write-Output ("  (still running: " + [bool](Get-Process -Id $proc.Id -ErrorAction SilentlyContinue) + ")")
  exit 4
}

Write-Output ""
Write-Output ("=== 4. settle " + $SettleSeconds + "s, then capture (no z-order or focus change) ===")
Start-Sleep -Seconds $SettleSeconds

$shots = @()
foreach ($id in $pids) {
  $vis = [WinCap]::VisibleMainWindows([uint32]$id)
  foreach ($h in $vis) {
    $f = Join-Path $OutDir ("coldstart-pid" + $id + "-" + $h.ToString("X") + ".png")
    # PrintWindow first: it is both occlusion-independent and non-intrusive.
    #
    # PrintWindow asks the window to render itself into a device context, so
    # whatever is stacked above it is irrelevant, and no z-order or focus change
    # is needed to capture this app's own pixels. An earlier version used
    # CopyFromScreen because PrintWindow returned solid black in a previous test
    # - but that test was on a *hidden* window. With the window visible, which is
    # the state a deep link produces, flag 2 (PW_RENDERFULLCONTENT) returns the
    # real frame. CopyFromScreen stays as the fallback, and its coverage is
    # reported because a screen grab of an occluded region is another window's
    # pixels: that produced a richly coloured frame of a text editor here, which a
    # "has content" test happily called usable.
    $cov = [WinCap]::Coverage($h, [uint32]$id)
    $ok = [WinCap]::CapturePrintWindow($h, $f)
    $method = "PrintWindow(PW_RENDERFULLCONTENT)"
    if ($ok) {
      $s0 = [WinCap]::Stats($f)
      if ($s0 -match 'min=(\d+) max=(\d+)' -and $Matches[1] -eq $Matches[2]) { $ok = $false }
    }
    if (-not $ok) {
      $ok = [WinCap]::CaptureScreen($h, $f, 800)
      $method = "CopyFromScreen(window rect) [PrintWindow was flat]"
    }
    if ($ok) {
      $stats = [WinCap]::Stats($f)
      $size = (Get-Item $f).Length
      Write-Output ("  " + [WinCap]::Describe($h))
      Write-Output ("    -> " + $f + "   " + $size + " bytes   " + $stats)
      Write-Output ("    method: " + $method + "   coverage: " + $cov)
      $shots += [pscustomobject]@{ Path = $f; Desc = [WinCap]::Describe($h); Pid = $id; Stats = $stats; Bytes = $size; Method = $method; Coverage = $cov }
    } else {
      Write-Output ("  " + [WinCap]::Describe($h) + " -> BOTH CAPTURE METHODS FAILED")
    }
  }
}
if ($shots.Count -eq 0) { Write-Output "  no visible main window to capture" }

# A capture has to pass two checks to count as evidence: it must have content
# (not a flat black frame) and it must actually be of this window (not of
# whatever is stacked above it). Either failure is reported as INCONCLUSIVE, so
# neither can be mistaken for proof that the dialog is on screen.
Write-Output ""
Write-Output "=== 4b. capture sanity ==="
Write-Output "  flat frame        -> INCONCLUSIVE (a black frame is not evidence)"
Write-Output "  occluded screen grab -> INCONCLUSIVE (the frame is another window's pixels)"
$usable = 0
foreach ($s in $shots) {
  $flat = $s.Stats -match 'min=(\d+) max=(\d+)' -and $Matches[1] -eq $Matches[2]
  $viaScreen = $s.Method -like "CopyFromScreen*"
  $own = [WinCap]::CoverageOwn($s.Coverage)
  $tot = [WinCap]::CoverageTotal($s.Coverage)
  # Coverage only matters for a screen grab: PrintWindow renders the window
  # itself, so overlapping windows are irrelevant to it.
  $occluded = ($viaScreen -and $own -ge 0 -and $tot -gt 0 -and $own -lt ($tot * 0.8))
  $name = Split-Path $s.Path -Leaf
  if ($flat) {
    Write-Output ("  " + $name + "  INCONCLUSIVE - flat single colour")
  } elseif ($occluded) {
    Write-Output ("  " + $name + "  INCONCLUSIVE - occluded (" + $s.Coverage + "); the frame shows another window, not the app")
  } else {
    Write-Output ("  " + $name + "  USABLE - " + $s.Method + "; coverage " + $s.Coverage + "; inspect it visually")
    $usable++
  }
}
Write-Output ("  usable captures: " + $usable + " / " + $shots.Count)
Write-Output ("  screenshot dir : " + $OutDir)

# -- 5. logs ----------------------------------------------------------------
Write-Output ""
Write-Output "=== 5. app logs ($env:USERPROFILE\.aimyria\logs) ==="
$logDir = Join-Path $env:USERPROFILE ".aimyria\logs"
if (Test-Path $logDir) {
  Get-ChildItem $logDir -File | Sort-Object LastWriteTime -Descending | Select-Object -First 3 |
    ForEach-Object { Write-Output ("  " + $_.Name + "  " + $_.Length + " bytes  " + $_.LastWriteTime) }
} else { Write-Output "  (no log directory)" }

Write-Output ""
Write-Output "=== captured files ==="
$shots | ForEach-Object { Write-Output ("  " + $_.Path + "   " + $_.Desc) }
Write-Output ("PID(s) left running: " + ((Get-AppPids) -join ", ") + "  (stop with: Get-Process aimyria-one | Stop-Process -Force)")
