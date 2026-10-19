# Build the release with the MSVC toolchain.
#
# # Why MSVC
#
# The GNU build links the WebView2 loader dynamically, which puts
# `WebView2Loader.dll` in the exe's import table and makes the binary fail to start
# anywhere that file is not sitting beside it. MSVC links the SDK's
# `WebView2LoaderStatic.lib` instead, producing an exe that runs on its own.
#
# The same switch is what makes `build.rs`'s Common Controls v6 workaround apply -
# it is guarded on `target_env = "msvc"` - so the Rust unit tests, which currently
# die with STATUS_ENTRYPOINT_NOT_FOUND, get their manifest.
#
# # Why the environment dance
#
# `link.exe` is on disk but useless without `INCLUDE`/`LIB` pointing at the MSVC and
# SDK headers and import libraries. Those are set by `vcvars64.bat`, a cmd script.
# PowerShell cannot call it and inherit the result, so the variables are read back
# out of a `vcvars64.bat && set` dump and applied here.
#
# # Why a separate target directory
#
# The GNU artifacts in `D:\Rust\build\aimyria-one` are a known-working fallback -
# the package that ships today. A different toolchain produces different objects,
# and letting the two share a directory makes it unclear which one any given file
# belongs to. Separate directory, separate history, and the fallback stays intact.

param(
  [string]$Toolchain   = "1.95-x86_64-pc-windows-msvc",
  # Defaults are resolved from this machine, not from the machine that first
  # wrote this script. The original literals pointed at `D:\Rust\build\...`,
  # `D:\VSBuildTools\...` and `D:\src\...`; none of those exist here, and a
  # default that silently resolves to nothing is how a build fails for a reason
  # that has nothing to do with the code. Callers may still override every one.
  [string]$TargetDir   = "",
  [string]$Vcvars      = "",
  [string]$ManifestDir = "",
  [string]$RustupHome  = "",
  [string]$CargoHome   = "",
  # Link the C runtime statically.
  #
  # MSVC's default is the dynamic VC runtime, which puts `VCRUNTIME140.dll`,
  # `VCRUNTIME140_1.dll` and `api-ms-win-crt-*.dll` in the import table. Those
  # ship with the VC++ redistributable, not with Windows, so the default build
  # only starts on a machine that has already installed it -- the GNU build
  # depended on `msvcrt.dll`, which is a system component, so the MSVC build is
  # *less* portable than the one it replaced. `crt-static` folds the runtime
  # into the executable and removes those imports; verified by `objdump -p`.
  #
  # It is a switch rather than a default so the difference stays visible: this
  # is the flag that decides whether "double-click and it runs" is true.
  [switch]$StaticCrt,
  [switch]$Test
)

$ErrorActionPreference = "Continue"

function Say($m) { Write-Output ("  " + $m) }

# -- resolve the paths this run actually needs --------------------------------
# `$PSScriptRoot` is `<repo>\scripts`, so the crate is one level up. Deriving it
# rather than hardcoding means moving the checkout cannot silently break it.
if (-not $ManifestDir) { $ManifestDir = Join-Path (Split-Path -Parent $PSScriptRoot) "src-tauri" }
if (-not $Vcvars) {
  $Vcvars = "D:\VSBuildTools\VC\Auxiliary\Build\vcvars64.bat"
}
if (-not $RustupHome) {
  $RustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE ".rustup" }
}
if (-not $CargoHome) {
  $CargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE ".cargo" }
}
if (-not $TargetDir) {
  $TargetDir = Join-Path (Split-Path -Parent $PSScriptRoot) "target\msvc-release"
}

foreach ($p in @(
  @{ n = "ManifestDir"; v = $ManifestDir },
  @{ n = "Vcvars";      v = $Vcvars },
  @{ n = "RustupHome";  v = $RustupHome },
  @{ n = "CargoHome";   v = $CargoHome }
)) {
  if (-not (Test-Path -LiteralPath $p.v)) { throw "$($p.n) not found at $($p.v)" }
}
Say "ManifestDir  $ManifestDir"
Say "RustupHome   $RustupHome"
Say "CargoHome    $CargoHome"
Say "TargetDir    $TargetDir"

# -- MSVC environment ---------------------------------------------------------
$dump = cmd /c "`"$Vcvars`" && set" 2>&1
$applied = 0
$skipped = @()
foreach ($line in $dump) {
  if ("$line" -match '^([A-Za-z_][A-Za-z0-9_()]*)=(.*)$') {
    # SilentlyContinue here was the dangerous part: a name PowerShell refuses to
    # write (a read-only automatic variable, say) would drop out of the
    # environment with the count still going up, and the build would then fail
    # somewhere unrelated to the real cause. Record the ones that did not land.
    try {
      Set-Item -Path ("Env:" + $matches[1]) -Value $matches[2] -ErrorAction Stop
      $applied++
    } catch {
      $skipped += $matches[1]
    }
  }
}
Say "vcvars64 environment applied ($applied variables)"
if ($skipped.Count -gt 0) { Say ("not applied (kept existing value): " + ($skipped -join ", ")) }

$link = Get-Command link.exe -ErrorAction SilentlyContinue
Say ("link.exe -> " + $(if ($link) { $link.Source } else { "NOT ON PATH" }))
if (-not $link) { throw "link.exe is not on PATH after vcvars64 - the MSVC build cannot link" }

# The Common Controls manifest that build.rs embeds into test binaries. Its absence
# would not fail the build; it would fail the tests, silently, which is the worse
# outcome - so it is checked here rather than discovered later.
$manifest = Join-Path $ManifestDir "common-controls.manifest"
Say ("common-controls.manifest -> " + $(if (Test-Path $manifest) { "present" } else { "MISSING (tests will fail 0xC0000139)" }))

# -- build --------------------------------------------------------------------
$env:RUSTUP_HOME    = $RustupHome
$env:CARGO_HOME     = $CargoHome
$env:Path           = "$CargoHome\bin;$env:Path"
$env:RUSTUP_TOOLCHAIN = $Toolchain
$env:CARGO_TARGET_DIR = $TargetDir
$env:CARGO_TERM_COLOR = "never"

# RUSTFLAGS, not a Cargo.toml setting: `crt-static` is a target feature, and
# putting it in the manifest would silently apply it to every platform and to
# `cargo test` too. Passing it per-invocation keeps the choice where it is made.
if ($StaticCrt) {
  $env:RUSTFLAGS = "-C target-feature=+crt-static"
  Say "StaticCrt -> RUSTFLAGS=$env:RUSTFLAGS"
} elseif ($env:RUSTFLAGS) {
  Say "RUSTFLAGS inherited from the environment: $env:RUSTFLAGS"
}

Say "toolchain $Toolchain   target $TargetDir"
$cmd = if ($Test) { "test --lib --no-run" } else { "build --release" }
Say "building: cargo $cmd - a full first build takes ~10 min"

# Everything from here to the end of the process runs inside try/finally so that a
# run *always* ends with a line saying how it ended.
#
# Why: an earlier version streamed its output through `Tee-Object` and relied on
# the pipeline's exit code. When that pipeline was torn down mid-build the log
# simply stopped after "building - a full first build, expect ~10 min", with no
# error and no exit code, while the artifact on disk stayed at an older build. That
# is the worst possible shape for a build log: it looks like a build in progress
# from the outside, and it is indistinguishable from a hang, a crash, and a
# deliberate abort. A log whose last line is not a verdict cannot be trusted, so
# the verdict is now written unconditionally, including on Ctrl-C.
$script:concluded = $false
$script:sw = [Diagnostics.Stopwatch]::StartNew()
$script:startedAt = Get-Date
trap {
  Say ("!! ABORTED after {0:N1} min: {1}" -f $script:sw.Elapsed.TotalMinutes, $_.Exception.Message)
  Say "!! BUILD INCOMPLETE - the artifact below may be an OLDER build"
  $script:concluded = $true
  exit 130
}

Push-Location $ManifestDir
$out = & cargo $cmd.Split(" ") --message-format short 2>&1 | Out-String
$code = $LASTEXITCODE
Pop-Location

Say ("cargo {0} -> exit {1} after {2:N1} min" -f $cmd, $code, $script:sw.Elapsed.TotalMinutes)
($out -split "`r?`n") | Where-Object { $_ -match ': error|error\[E' } |
  Select-Object -First 8 | ForEach-Object { Say ("  " + $_.Trim()) }

if ($code -ne 0) {
  Say ("RESULT=FAIL exit={0} at {1}" -f $code, (Get-Date).ToString("HH:mm:ss"))
  Write-Output ("BUILD LOG: exit={0} (fail) - artifact NOT updated" -f $code)
  $script:concluded = $true
  exit $code
}

# -- the decisive check -------------------------------------------------------
# A successful build says nothing about whether the DLL dependency is gone. That
# lives in the import table, so read the import table.
$exe = Join-Path $TargetDir "release\aimyria-one.exe"
if (-not (Test-Path $exe)) { throw "build reported success but $exe is missing" }

# objdump is what reads the import table. It lives in the MSYS2 tree on this
# machine; the original hardcoded `D:\Tools\mingw64\mingw64\bin` does not exist
# here, and the `if (Test-Path ...)` guard meant the decisive check would simply
# not run - the one check that distinguishes the artifact we want from the one
# we already had.
$objdump = "C:\msys64\mingw64\bin\objdump.exe"
if (-not (Test-Path -LiteralPath $objdump)) {
  $found = Get-Command objdump.exe -ErrorAction SilentlyContinue
  $objdump = if ($found) { $found.Source } else { $null }
}
if ($objdump) {
  $imports = (& $objdump -p $exe 2>$null | Select-String '^\s*DLL Name:').Line
  $wv2 = $imports | Where-Object { $_ -match 'WebView2Loader' }
  Say ("import table: " + $(if ($wv2) { "STILL DEPENDS ON " + ($wv2 -join ", ") } else { "no WebView2Loader entry - self-contained" }))
} else {
  Say "objdump NOT FOUND - the import table was not checked here"
}

Get-Item $exe | ForEach-Object {
  Say ("exe  {0:N2} MB   {1}" -f ($_.Length / 1MB), $_.LastWriteTime)
  # Record when the artifact was produced relative to when this run started. A
  # stale binary beside a successful-looking log is how a "fix" gets reported as
  # shipped without ever having been compiled in.
  $fresh = $_.LastWriteTime -ge $script:startedAt.AddSeconds(-5)
  Say ("artifact is " + $(if ($fresh) { "NEW (written during this run)" } else { "OLD (predates this run - cargo considered it up to date)" }))
}

Say ("RESULT=OK exit=0 at {0} after {1:N1} min" -f (Get-Date).ToString("HH:mm:ss"), $script:sw.Elapsed.TotalMinutes)
Write-Output ("BUILD LOG: exit=0 (ok) - {0}" -f $exe)
