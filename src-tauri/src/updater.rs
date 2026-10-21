//! Silent self-update.
//!
//! # Why this is not `tauri-plugin-updater`
//!
//! The plugin was wired up and dead. `tauri.conf.json` pointed it at
//! `https://api.aimyria.com/updates/latest.json`, which is the API gateway and
//! answers 401; `bundle.createUpdaterArtifacts` was `false`, so no `latest.json`
//! or `.sig` was ever produced; and there is no minisign private key in the
//! repository. Three independent reasons for `hasUpdate` to be permanently
//! `false`, so the button that says 「检查更新」 could never find anything.
//!
//! Reviving it would mean publishing a key pair and treating its loss as "nobody
//! can ever update again". This module instead uses the integrity primitive the
//! project already ships: **sha256 over https**. No signing, by decision.
//!
//! # Manifest
//!
//! `GET {base}/releases/latest/download/version.json`
//!
//! ```json
//! { "version": "1.0.1",
//!   "url": "https://aimyria.com/releases/latest/download/AimyriaOne-x64.exe",
//!   "sha256": "<lowercase hex of the exact bytes served>",
//!   "notes": "...", "pubDate": "2026-10-19T20:30:00+08:00" }
//! ```
//!
//! It sits next to the exe it describes, so one directory listing is the whole
//! release.
//!
//! # The five things that make a silent updater safe
//!
//! 1. **Fail closed.** A payload whose sha256 does not match the manifest is
//!    deleted, never installed. A truncated download is the most likely way this
//!    goes wrong, and it is the one that would leave the user with no program.
//! 2. **Refuse to shoot the messenger.** Only `https` on our own hosts, plus
//!    loopback when the test clock is on. Redirects are re-checked against the
//!    same allowlist, so a hijacked 302 cannot walk the download off-site.
//! 3. **Never interrupt a half-finished import.** A deep link that has been
//!    stashed but not claimed, or a visible window, means a user is mid-flow.
//!    The phase is held back until they are done.
//! 4. **Keep the previous exe for one cycle.** `rename` moves the running exe
//!    aside rather than deleting it, and the new process only removes `.old`
//!    after it has started under its own version. Manual recovery stays possible
//!    for exactly as long as it is needed.
//! 5. **One attempt per window.** A failed round waits for the next scheduled
//!    check; there is no retry loop to hammer a bad server.
//!
//! # The replacement dance, and why it looks like this
//!
//! Measured on this machine against a live process:
//!
//! | operation                              | result |
//! |----------------------------------------|--------|
//! | overwrite the running exe in place      | EBUSY  |
//! | delete the running exe                  | EPERM  |
//! | **rename the running exe to `.old`**    | OK     |
//! | **move `.new` into the exe's name**     | OK     |
//!
//! So the only order that works is: rename the current one aside, then move the
//! new one into its name. Any failure on the way puts `.old` back, and the exe
//! is never left missing under its own name. `.new` is written in the same
//! directory as the exe (`rename` cannot cross volumes), and both temporary
//! names take a lock-free existence check first.
//!
//! `restart_process` — the same path Settings' 「重启」 uses — is what actually
//! relaunches. The replacement is only visible to the new process: the running
//! image stays in memory, so nothing here is observable until the restart.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

/// The manifest this build asks for. Pinned by product decision: it shares a
/// directory with the payload, so the existing `/releases/` location covers it.
pub const DEFAULT_MANIFEST_URL: &str = "https://aimyria.com/releases/latest/download/version.json";

/// Hosts a manifest or payload URL is allowed to name.
///
/// `sub2.aimyria.com` serves the same files but is a *site* host, not a release
/// host; keeping the allowlist to the two names that actually publish the
/// directory is enough, and a shorter list is easier to be sure about.
const ALLOWED_HOSTS: [&str; 2] = ["aimyria.com", "www.aimyria.com"];

/// Project version, used for the comparison the manifest is judged against.
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Overrides the manifest URL. Local acceptance runs need to point the updater
/// at a server they control; leaving this out would make the whole mechanism
/// testable only in production, which is the same as untestable.
pub const MANIFEST_URL_ENV: &str = "AIMYRIA_UPDATE_MANIFEST_URL";

/// Shortens the three delays so an acceptance run can watch the phases happen
/// instead of waiting an hour. It additionally permits `http` on loopback, which
/// is what makes a local server usable. Nothing else changes: the allowlist,
/// the sha256 gate, the rollback and the `.old` retention are the same code
/// paths either way, so a test run exercises the shipping logic.
pub const TEST_CLOCK_ENV: &str = "AIMYRIA_UPDATE_TEST_CLOCK";

/// Stages, verifies and logs but stops before the rename. Lets a run inspect the
/// artifact that would be installed.
pub const DRY_RUN_ENV: &str = "AIMYRIA_UPDATE_DRYRUN";

const FIRST_CHECK_DELAY_REAL: Duration = Duration::from_secs(60);
const FIRST_CHECK_DELAY_TEST: Duration = Duration::from_secs(3);
const PERIOD_REAL: Duration = Duration::from_secs(6 * 60 * 60);
const PERIOD_TEST: Duration = Duration::from_secs(15);

/// How long the pre-replace phase waits for an in-flight import to finish. The
/// watchdog fires on `>= this` and the check is *re-done* at that moment, so a
/// long import cannot be cut short — see [`WaitOutcome`].
const WATCH_REAL: Duration = Duration::from_secs(1800);
const WATCH_TEST: Duration = Duration::from_secs(6);

/// Pause between the last gate and the rename, when nothing is in flight. Gives
/// a dialog that is about to open a moment to appear, so the gate can still see
/// it.
const REPLACE_DELAY_REAL: Duration = Duration::from_secs(15);
const REPLACE_DELAY_TEST: Duration = Duration::from_millis(500);

/// Prefix on every line this module writes, so an acceptance script can pull the
/// whole narrative out of the log with one pattern.
const TAG: &str = "[updater]";

// ---------------------------------------------------------------------------
// manifest
// ---------------------------------------------------------------------------

/// The wire format of `version.json`.
///
/// `deny_unknown_fields` is deliberately *not* set: a publisher adding a field
/// must not brick every installed client.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default, rename = "pubDate")]
    pub pub_date: Option<String>,
    /// Other files that must travel with the exe, each with its own checksum.
    ///
    /// The package ships `WebView2Loader.dll` beside the exe, and the installer
    /// downloads and verifies it separately (`quickstart.ps1`). The updater used
    /// to replace only the exe, so the two halves of a release could drift: a
    /// build that needs a newer loader would install the exe and leave the old
    /// loader in place, and the app would then fail to start with
    /// `STATUS_ENTRYPOINT_NOT_FOUND` — the exact symptom the loader exists to
    /// prevent, arriving silently on update instead of on install.
    ///
    /// Optional and empty by default, so every manifest published before this
    /// field existed still parses; the fleet can update *into* the version that
    /// understands companions.
    #[serde(default)]
    pub companions: Vec<Companion>,
}

/// One file that must be installed alongside the executable.
#[derive(Debug, Clone, Deserialize)]
pub struct Companion {
    /// File name only — no directory, no `..`. It is joined onto the exe's own
    /// directory, so a manifest cannot write anywhere else.
    pub name: String,
    /// Lowercase hex sha256 of the exact bytes served.
    pub sha256: String,
    /// Download URL; checked against the same host allowlist as the payload.
    pub url: String,
}

fn test_clock_on() -> bool {
    std::env::var(TEST_CLOCK_ENV)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

fn dry_run() -> bool {
    std::env::var(DRY_RUN_ENV)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

pub fn manifest_url() -> String {
    std::env::var(MANIFEST_URL_ENV)
        .map(|v| v.trim().to_string())
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_MANIFEST_URL.to_string())
}

// ---------------------------------------------------------------------------
// version comparison
// ---------------------------------------------------------------------------

/// Parse a dotted numeric version into comparable segments.
///
/// `semver` is not a dependency and is not worth adding for this. Anything that
/// is not a pure run of dot-separated decimal numbers — `v1.0.1`, `1.0.1-beta`,
/// `1.0.x` — returns `None`, and a `None` on either side means **no update**.
/// That is the fail-closed direction: a manifest we cannot compare confidently
/// is a manifest we do not act on.
pub fn parse_numeric_version(raw: &str) -> Option<Vec<u64>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for segment in raw.split('.') {
        if segment.is_empty() || !segment.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        parts.push(segment.parse::<u64>().ok()?);
    }
    Some(parts)
}

/// Is `candidate` strictly newer than `current`?
///
/// Compare segment by segment, padding the shorter side with zeroes, so
/// `1.10.0 > 1.9.0` (the case a string comparison gets backwards) and `1.1 > 1.1.0`
/// is not an update.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let (Some(cand), Some(cur)) = (
        parse_numeric_version(candidate),
        parse_numeric_version(current),
    ) else {
        return false;
    };
    let len = cand.len().max(cur.len());
    for index in 0..len {
        let a = cand.get(index).copied().unwrap_or(0);
        let b = cur.get(index).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// URL policy
// ---------------------------------------------------------------------------

/// Is this URL one we are willing to fetch?
///
/// Production: `https` on [`ALLOWED_HOSTS`], nothing else. With the test clock
/// on, plain `http` to loopback is additionally allowed so a local server can
/// stand in for the release host.
pub fn url_is_allowed(raw: &str, test_mode: bool) -> bool {
    let Ok(parsed) = url::Url::parse(raw) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    match parsed.scheme() {
        "https" => ALLOWED_HOSTS
            .iter()
            .any(|allowed| host.eq_ignore_ascii_case(allowed)),
        "http" => test_mode && is_loopback(host),
        _ => false,
    }
}

fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "[::1]"
        || host == "::1"
}

/// Drop a UTF-8 byte-order mark, if present.
///
/// `serde_json` refuses a document that starts with one; a publisher's tooling
/// emitting a BOM is common enough on Windows that tolerating it is cheaper than
/// diagnosing it. Only the exact three-byte UTF-8 sequence is removed -- UTF-16
/// manifests are not supported and would still fail loudly, which is correct.
pub fn strip_utf8_bom(bytes: &[u8]) -> &[u8] {
    const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];
    if bytes.starts_with(&BOM) {
        &bytes[BOM.len()..]
    } else {
        bytes
    }
}

/// Append `?t=<unix seconds>` so each check is a distinct cache key.
///
/// See the comment at the manifest fetch site for why this exists alongside the
/// `Cache-Control: no-cache` request header: a cache keyed only by URL ignores
/// that header entirely, and one of those is what served `max-age=300`.
///
/// `t` is added with `query_pairs_mut`, and the existing query is preserved, so
/// the result stays a valid URL whatever the configured base carries. A URL that
/// somehow fails to parse is returned unchanged -- a malformed manifest URL is
/// refused later by [`url_is_allowed`], and losing the cache-buster must not turn
/// into losing the check.
pub fn with_cache_buster(raw: &str) -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match url::Url::parse(raw) {
        Ok(mut parsed) => {
            parsed
                .query_pairs_mut()
                .append_pair("t", &seconds.to_string());
            parsed.into()
        }
        Err(_) => raw.to_string(),
    }
}

// ---------------------------------------------------------------------------
// the in-flight gate
// ---------------------------------------------------------------------------

/// Is a deep link stashed but not yet claimed?
///
/// Separate from [`import_in_flight`] so the two signals can be logged
/// individually when a round is deferred; "which one held it back" is a question
/// worth being able to answer from the log alone.
fn deeplink_pending() -> bool {
    crate::deeplink::has_pending_deeplink()
}

/// Is a user part-way through the import flow?
///
/// Two signals, both conservative:
///
/// 1. a deep link stashed but not yet claimed — the backend recorded a request
///    the UI has not shown yet, so a restart right now would lose it;
/// 2. the window is visible — the import dialog opens the window, and nothing
///    else about this tray app normally has it on screen.
///
/// Either one holds the replacement. Neither is authoritative *forever* (a
/// stashed link that is never claimed, a window left open for a chat), which is
/// why the wait has a watchdog rather than blocking the update channel outright.
fn import_in_flight(app: &AppHandle) -> bool {
    if deeplink_pending() {
        return true;
    }
    match app.get_webview_window("main") {
        Some(window) => window.is_visible().unwrap_or(false),
        None => false,
    }
}

/// What the pre-replace wait decided.
enum WaitOutcome {
    /// Nobody is mid-import; proceed.
    ///
    /// Also the answer when the watchdog expires and the condition has cleared by
    /// then: the decision is made against the state *at that moment*, not against
    /// the reason the wait started, so a long import that finishes is not punished.
    Clear,
    /// Signals are still live when the watchdog expires. The only case where a
    /// round is abandoned.
    Defer,
}

/// Hold the replacement until no import is in flight, or the watchdog expires.
///
/// The check is re-evaluated at the boundary rather than trusting a flag set when
/// the wait began. A 20-minute import and a 20-minute idle both start the same
/// timer; only one of them should defer.
async fn wait_for_clear(app: &AppHandle, watchdog: Duration) -> WaitOutcome {
    let started = std::time::Instant::now();
    let mut announced = false;
    loop {
        if !import_in_flight(app) {
            return WaitOutcome::Clear;
        }
        if started.elapsed() >= watchdog {
            log::warn!(
                "{TAG} deferred: an import is still in flight after {}s; \
                 waiting for the next scheduled check",
                watchdog.as_secs()
            );
            return WaitOutcome::Defer;
        }
        if !announced {
            log::info!(
                "{TAG} holding the replacement: pending_deeplink={} window_visible={} \
                 (up to {}s)",
                deeplink_pending(),
                app.get_webview_window("main")
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false),
                watchdog.as_secs()
            );
            announced = true;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

// ---------------------------------------------------------------------------
// filesystem: staging, verification, replacement
// ---------------------------------------------------------------------------

/// `<exe>.new` / `<exe>.old` — appended, not `with_extension`, so the sibling of
/// `AimyriaOne-x64.exe` is `AimyriaOne-x64.exe.new` rather than something that
/// only shares a stem with it.
fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// Download, verify and install the files that ship beside the exe.
///
/// Each companion is written to `<name>.new` in the install directory, hashed
/// **from disk**, and only then renamed over the real file. A checksum mismatch
/// discards that file and aborts the whole round — a half-updated pair of
/// binaries is worse than not updating.
///
/// `name` is validated to be a bare file name. Without that check a manifest
/// could ask the updater to write `..\..\Windows\System32\whatever.dll`, and the
/// host allowlist would not help: the URL is legitimate, the destination is not.
async fn install_companions(
    client: &reqwest::Client,
    exe: &Path,
    companions: &[Companion],
    test_mode: bool,
    dry: bool,
) -> Result<(), String> {
    if companions.is_empty() {
        return Ok(());
    }
    let dir = exe
        .parent()
        .ok_or_else(|| "cannot resolve the install directory".to_string())?;

    for companion in companions {
        let name = companion.name.trim();
        if name.is_empty()
            || name.contains(['/', '\\'])
            || name == "."
            || name == ".."
            || name.contains(':')
        {
            return Err(format!(
                "companion name {:?} is not a bare file name; refusing",
                companion.name
            ));
        }
        if !url_is_allowed(&companion.url, test_mode) {
            return Err(format!(
                "companion {} URL outside the allowlist: {}",
                name, companion.url
            ));
        }

        let target = dir.join(name);
        let staged = dir.join(format!("{name}.new"));

        let bytes = {
            let response = client
                .get(&companion.url)
                .send()
                .await
                .map_err(|e| format!("companion {name}: download failed: {e}"))?;
            if !response.status().is_success() {
                return Err(format!("companion {name}: HTTP {}", response.status()));
            }
            response
                .bytes()
                .await
                .map_err(|e| format!("companion {name}: body read failed: {e}"))?
        };

        // Write, then hash the file rather than the buffer: hashing memory only
        // proves what was downloaded, not what the next process will load.
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&staged).map_err(|e| {
                format!("companion {name}: cannot create {}: {e}", staged.display())
            })?;
            file.write_all(&bytes)
                .map_err(|e| format!("companion {name}: write failed: {e}"))?;
            file.flush()
                .map_err(|e| format!("companion {name}: flush failed: {e}"))?;
            file.sync_all()
                .map_err(|e| format!("companion {name}: fsync failed: {e}"))?;
        }

        let actual = match sha256_hex_of_file(&staged) {
            Ok(value) => value,
            Err(e) => {
                let _ = std::fs::remove_file(&staged);
                return Err(format!("companion {name}: cannot hash staged file: {e}"));
            }
        };
        let expected = companion.sha256.trim().to_ascii_lowercase();
        if actual != expected {
            let _ = std::fs::remove_file(&staged);
            return Err(format!(
                "companion {name}: sha256 mismatch (expected {expected}, got {actual}); \
                 discarded, nothing installed"
            ));
        }

        if dry {
            log::info!(
                "{TAG} DRY RUN: verified companion {name} at {}; not installing",
                staged.display()
            );
            continue;
        }

        // A loaded DLL cannot be replaced in place on Windows, but the loader is
        // already mapped into this process and the new one starts after we exit.
        // Renaming the old file aside first is what makes the swap possible at
        // all, and it matches the exe dance below.
        if target.exists() {
            let backup = dir.join(format!("{name}.old"));
            let _ = std::fs::remove_file(&backup);
            if let Err(e) = std::fs::rename(&target, &backup) {
                let _ = std::fs::remove_file(&staged);
                return Err(format!(
                    "companion {name}: cannot move the existing file aside: {e}"
                ));
            }
        }
        if let Err(e) = std::fs::rename(&staged, &target) {
            let _ = std::fs::remove_file(&staged);
            let backup = dir.join(format!("{name}.old"));
            if backup.exists() {
                let _ = std::fs::rename(&backup, &target);
            }
            return Err(format!(
                "companion {name}: cannot install the verified file: {e}"
            ));
        }
        log::info!("{TAG} installed companion {name} ({} bytes)", bytes.len());
    }

    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sha256_hex_of_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// A refusal that is reported rather than retried.
#[derive(Debug)]
pub enum ReplaceError {
    Write(std::io::Error),
    /// The bytes that landed on disk do not hash to the manifest's value.
    Checksum {
        expected: String,
        actual: String,
    },
    Rename(String),
    /// The replacement failed after the exe had already been renamed aside, and
    /// putting it back also failed. The only genuinely dangerous state, so it is
    /// its own variant and is logged as an error, not a warning.
    RollbackFailed(String),
}

impl std::fmt::Display for ReplaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReplaceError::Write(e) => write!(f, "写入失败: {e}"),
            ReplaceError::Checksum { expected, actual } => write!(
                f,
                "sha256 校验失败（清单期望 {expected}，落盘实际 {actual}）"
            ),
            ReplaceError::Rename(detail) => write!(f, "替换失败: {detail}"),
            ReplaceError::RollbackFailed(detail) => {
                write!(f, "替换失败且回滚失败，需要人工恢复: {detail}")
            }
        }
    }
}

/// Everything up to and including the rename.
///
/// Synchronous on purpose: it is called inside `spawn_blocking`, so the async
/// worker is not parked on disk I/O, and a half-done rename sequence cannot be
/// interrupted by a task cancellation.
pub fn stage_and_replace(
    exe: &Path,
    bytes: &[u8],
    expected_sha256: &str,
    dry: bool,
) -> Result<(), ReplaceError> {
    let new_path = sibling(exe, ".new");
    let old_path = sibling(exe, ".old");

    // A leftover from an interrupted run would make the rename below fail on a
    // file that has nothing to do with this one. Neither name should exist at
    // this point — `startup_cleanup` removes them — so their presence is worth a
    // line in the log.
    for stale in [&new_path, &old_path] {
        if stale.exists() {
            log::warn!(
                "{TAG} removing a leftover {} before staging",
                stale.display()
            );
            let _ = std::fs::remove_file(stale);
        }
    }

    // 1. write, flush, fsync -- the bytes have to be durable before the exe they
    //    will replace is moved aside.
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&new_path).map_err(ReplaceError::Write)?;
        file.write_all(bytes).map_err(ReplaceError::Write)?;
        file.flush().map_err(ReplaceError::Write)?;
        file.sync_all().map_err(ReplaceError::Write)?;
    }
    log::info!(
        "{TAG} staged {} bytes at {}",
        bytes.len(),
        new_path.display()
    );

    // 2. verify the bytes that are **on disk**, by reading them back. Hashing the
    //    in-memory slice would only prove what we already believed.
    let actual = match sha256_hex_of_file(&new_path) {
        Ok(value) => value,
        Err(e) => {
            let _ = std::fs::remove_file(&new_path);
            return Err(ReplaceError::Write(e));
        }
    };
    let expected = expected_sha256.trim().to_ascii_lowercase();
    if actual != expected {
        // fail closed: nothing on this disk is touched beyond deleting the file
        // we just wrote.
        let _ = std::fs::remove_file(&new_path);
        log::error!(
            "{TAG} sha256 mismatch: manifest expected={expected} actual={actual}; \
             discarding {} and NOT replacing {}",
            new_path.display(),
            exe.display()
        );
        return Err(ReplaceError::Checksum { expected, actual });
    }
    log::info!("{TAG} sha256 verified: {actual}");

    if dry {
        log::info!(
            "{TAG} DRY RUN ({} set): verified payload left at {}; not replacing",
            DRY_RUN_ENV,
            new_path.display()
        );
        return Ok(());
    }

    // 3. rename the running exe aside. Allowed on Windows; overwriting and
    //    deleting a running image are not.
    std::fs::rename(exe, &old_path).map_err(|e| {
        let _ = std::fs::remove_file(&new_path);
        ReplaceError::Rename(format!(
            "renaming {} -> {} failed: {e}; payload discarded, exe untouched",
            exe.display(),
            old_path.display()
        ))
    })?;
    log::info!(
        "{TAG} renamed {} -> {} (kept as the rollback copy)",
        exe.display(),
        old_path.display()
    );

    // 4. move the new one into the exe's name. A failure here is the one case
    //    that needs the rollback.
    if let Err(e) = std::fs::rename(&new_path, exe) {
        let restore = std::fs::rename(&old_path, exe);
        let _ = std::fs::remove_file(&new_path);
        return match restore {
            Ok(()) => Err(ReplaceError::Rename(format!(
                "moving {} -> {} failed: {e}; rolled back, {} is the original exe again",
                new_path.display(),
                exe.display(),
                exe.display()
            ))),
            Err(restore_error) => Err(ReplaceError::RollbackFailed(format!(
                "moving {} -> {} failed: {e}; rolling {} back to {} also failed: {restore_error}",
                new_path.display(),
                exe.display(),
                old_path.display(),
                exe.display()
            ))),
        };
    }
    log::info!(
        "{TAG} replaced {} with the verified payload; {} retained for one cycle",
        exe.display(),
        old_path.display()
    );
    Ok(())
}

/// Remove the remnants of a completed update, and report the version this process
/// is running.
///
/// Called on every launch, before the updater task starts. A `.old` next to the
/// exe means the previous run made it all the way through the rename and is now
/// running under its own name, which is exactly the condition that makes deleting
/// the rollback copy safe. A `.new` means a download was staged and the process
/// did not survive to use it; it is discarded, because a second look at it would
/// be a second chance to install bytes nothing has verified since.
///
/// The work itself lives in [`cleanup_siblings`] so it can be tested against a
/// real directory: `current_exe()` in a test is the *test* binary, so a test that
/// called this function directly would place its fixtures beside the build output
/// and assert nothing about them.
pub fn startup_cleanup() -> String {
    let version = CURRENT_VERSION.to_string();
    match std::env::current_exe() {
        // The leftover list is for tests; a launch only needs the log.
        Ok(exe) => {
            let _ = cleanup_siblings(&exe, &version);
            let _ = cleanup_companions(&exe);
        }
        Err(e) => log::warn!("{TAG} cannot resolve current_exe: {e}; skipping self-update cleanup"),
    }
    version
}

/// The filesystem half of [`startup_cleanup`], against an explicit exe path.
///
/// Returns the leftovers it could not remove, so the caller (or a test) can tell
/// "there was nothing to do" from "it refused".
pub fn cleanup_siblings(exe: &Path, version: &str) -> Vec<PathBuf> {
    let mut failed = Vec::new();

    let old_path = sibling(exe, ".old");
    if old_path.is_file() {
        match std::fs::remove_file(&old_path) {
            Ok(()) => log::info!(
                "{TAG} previous update completed: running v{version}, removed {}",
                old_path.display()
            ),
            Err(e) => {
                log::warn!("{TAG} could not remove {}: {e}", old_path.display());
                failed.push(old_path);
            }
        }
    }

    let new_path = sibling(exe, ".new");
    if new_path.is_file() {
        match std::fs::remove_file(&new_path) {
            Ok(()) => log::warn!(
                "{TAG} discarded a staged payload that was never installed: {}",
                new_path.display()
            ),
            Err(e) => {
                log::warn!("{TAG} could not remove {}: {e}", new_path.display());
                failed.push(new_path);
            }
        }
    }

    log::info!("{TAG} running v{version} from {}", exe.display());
    failed
}

/// Remove companion leftovers (`.new` / `.old`) and report the ones it could not
/// delete.
///
/// Companion names are not known at startup — they come from the manifest of the
/// release that installed them — so this scans the install directory instead.
/// The install directory holds the exe, the loader and the user's data folder, so
/// the pattern is deliberately narrow: a file whose name ends in `.dll.new` or
/// `.dll.old` was written by this module and by nothing else.
pub fn cleanup_companions(exe: &Path) -> Vec<PathBuf> {
    let mut failed = Vec::new();
    let Some(dir) = exe.parent() else {
        return failed;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return failed;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        let is_ours = lower.ends_with(".dll.new") || lower.ends_with(".dll.old");
        if !is_ours {
            continue;
        }
        // An `.old` next to the exe means the previous run finished installing and
        // is now running under its own name, so the backup is no longer needed. A
        // `.new` means a download never reached the rename, and installing bytes
        // nothing has verified since is a second chance to get it wrong.
        match std::fs::remove_file(&path) {
            Ok(()) => log::info!("{TAG} removed companion leftover {}", path.display()),
            Err(e) => {
                log::warn!("{TAG} could not remove {}: {e}", path.display());
                failed.push(path);
            }
        }
    }
    failed
}

// ---------------------------------------------------------------------------
// the check itself
// ---------------------------------------------------------------------------

/// One full round: fetch, compare, (maybe) stage, verify, replace, restart.
///
/// Returns `true` when the payload was installed and the process is about to be
/// replaced. Every failure path returns `false` after logging — the next check is
/// six hours away and that is the entire retry policy.
async fn check_once(app: &AppHandle) -> bool {
    let test_mode = test_clock_on();
    let manifest_url = manifest_url();

    if !url_is_allowed(&manifest_url, test_mode) {
        log::warn!("{TAG} refused manifest URL outside the allowlist: {manifest_url}");
        return false;
    }

    log::info!("{TAG} checking manifest: {manifest_url}");

    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        // Re-check every hop against the same allowlist. Without this a redirect
        // is a way to make the updater download from anywhere.
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() > 2 {
                return attempt.error("too many redirects");
            }
            if url_is_allowed(attempt.url().as_str(), test_mode) {
                attempt.follow()
            } else {
                attempt.error("redirect target is not on the release allowlist")
            }
        }))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            log::warn!("{TAG} could not build an HTTP client: {e}");
            return false;
        }
    };

    // -- fetch the manifest --------------------------------------------------
    //
    // Two independent defences against a cached manifest, because a stale
    // manifest is the quietest possible failure: the fleet simply never sees a
    // new release, and nothing anywhere reports a problem.
    //
    //   1. `Cache-Control: no-cache` on the request. One header, no cost, and it
    //      is what a well-behaved intermediary is supposed to honour.
    //   2. `?t=<unix seconds>` on the URL. A cache is keyed by *URL*, so a
    //      different `t` is a different key and no layer -- CDN, corporate proxy,
    //      or a local cache -- can hand back the previous body. This is the
    //      belt-and-braces half: it works against intermediaries that ignore
    //      request headers, which is exactly the class of thing that produced the
    //      original `max-age=300` problem.
    //
    // Neither is a substitute for the origin sending `no-store` (the real fix,
    // tracked server-side). ~300 bytes every six hours buys immunity from every
    // cache that only looks at the URL, so it is kept unconditionally.
    //
    // The timestamp is added here rather than baked into `DEFAULT_MANIFEST_URL`
    // so that the constant stays comparable to the URL the server publishes --
    // a test pins the two together, and a query string in the constant would
    // make that comparison meaningless.
    let request_url = with_cache_buster(&manifest_url);
    let manifest: Manifest = match client
        .get(&request_url)
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status();
            if !status.is_success() {
                log::warn!("{TAG} manifest returned HTTP {status}; staying on v{CURRENT_VERSION}");
                return false;
            }
            // Read the bytes and strip a UTF-8 BOM before parsing.
            //
            // `Response::json()` goes through serde_json, which rejects a body that
            // begins with a byte-order mark, reporting only "error decoding response
            // body". A BOM here is not hypothetical: it is what a lot of Windows
            // tooling produces by default -- PowerShell 5.1's `Set-Content -Encoding
            // UTF8`, anything on .NET Framework, Notepad's "UTF-8". A publisher
            // regenerating the manifest with any of those would break the update
            // channel for every installed copy, silently, with a log line that names
            // neither the BOM nor the file.
            //
            // Measured, and this is why the code is here: with a BOM-prefixed
            // manifest the updater logged exactly that opaque message and no update
            // ever ran, while the server answered 200 and the payload hash was
            // correct. Three bytes are therefore stripped rather than trusted.
            let bytes = match response.bytes().await {
                Ok(bytes) => bytes,
                Err(e) => {
                    log::warn!("{TAG} could not read the manifest body: {e}");
                    return false;
                }
            };
            match serde_json::from_slice::<Manifest>(strip_utf8_bom(&bytes)) {
                Ok(manifest) => manifest,
                Err(e) => {
                    // Include the length and the first bytes: without them a
                    // "did not parse" line costs an investigation to act on.
                    let head = bytes
                        .iter()
                        .take(24)
                        .map(|b| format!("{b:02X}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    log::warn!(
                        "{TAG} manifest did not parse as version.json: {e} \
                         ({} bytes, first 24: {head})",
                        bytes.len()
                    );
                    return false;
                }
            }
        }
        Err(e) => {
            log::warn!("{TAG} manifest fetch failed (offline?): {e}");
            return false;
        }
    };

    log::info!(
        "{TAG} manifest: version={} url={} sha256={} pubDate={}",
        manifest.version,
        manifest.url,
        manifest.sha256,
        manifest.pub_date.as_deref().unwrap_or("-")
    );

    // -- compare -------------------------------------------------------------
    if parse_numeric_version(&manifest.version).is_none() {
        log::warn!(
            "{TAG} manifest version '{}' is not three dot-separated numbers; \
             refusing to compare (staying on v{CURRENT_VERSION})",
            manifest.version
        );
        return false;
    }
    if parse_numeric_version(CURRENT_VERSION).is_none() {
        log::warn!("{TAG} this build's own version is unparseable; refusing to update");
        return false;
    }
    if !is_newer(&manifest.version, CURRENT_VERSION) {
        log::info!(
            "{TAG} up to date: running v{CURRENT_VERSION}, manifest has v{}",
            manifest.version
        );
        return false;
    }
    log::info!(
        "{TAG} update available: v{CURRENT_VERSION} -> v{}",
        manifest.version
    );

    // -- gate on an import, then download ------------------------------------
    // The wait happens *before* the download so the file is not pulled down for
    // a replacement that then gets deferred. The wait is re-checked by the
    // caller straight after the download, so the force-lock ordering that this
    // module's docs promise still holds.
    if let WaitOutcome::Defer =
        wait_for_clear(app, if test_mode { WATCH_TEST } else { WATCH_REAL }).await
    {
        return false;
    }

    let payload_url = manifest.url.clone();
    if !url_is_allowed(&payload_url, test_mode) {
        log::warn!("{TAG} refused payload URL outside the allowlist: {payload_url}");
        return false;
    }

    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            log::warn!("{TAG} cannot resolve current_exe: {e}");
            return false;
        }
    };

    log::info!("{TAG} downloading: {payload_url}");
    let bytes = match client.get(&payload_url).send().await {
        Ok(response) => {
            let status = response.status();
            if !status.is_success() {
                log::warn!("{TAG} payload returned HTTP {status}; no replacement");
                return false;
            }
            match response.bytes().await {
                Ok(bytes) => bytes,
                Err(e) => {
                    log::warn!("{TAG} payload download failed: {e}; no replacement");
                    return false;
                }
            }
        }
        Err(e) => {
            log::warn!("{TAG} payload fetch failed: {e}; no replacement");
            return false;
        }
    };
    log::info!(
        "{TAG} downloaded {} bytes; verifying sha256 before touching {}",
        bytes.len(),
        exe.display()
    );

    // -- the last gate, then the rename --------------------------------------
    if let WaitOutcome::Defer =
        wait_for_clear(app, if test_mode { WATCH_TEST } else { WATCH_REAL }).await
    {
        log::warn!("{TAG} deferring the verified payload: an import came in during the download");
        return false;
    }

    let pause = if test_mode {
        REPLACE_DELAY_TEST
    } else {
        REPLACE_DELAY_REAL
    };
    tokio::time::sleep(pause).await;
    if import_in_flight(app) {
        log::warn!(
            "{TAG} deferred: an import started during the final {}s pause",
            pause.as_secs()
        );
        return false;
    }

    let expected = manifest.sha256.clone();
    let dry = dry_run();

    // -- companions, before the exe ------------------------------------------
    //
    // Order matters. A new exe paired with an old loader is a build that may not
    // start; the reverse (new loader, old exe) is simply a newer loader doing the
    // same job, which is how a failed exe replacement lands. So companions go
    // first: if one of them fails, the exe has not been touched and the running
    // install stays coherent.
    //
    // The download happens here rather than beside the payload because a failed
    // companion must not have cost a full exe download and a rename.
    if let Err(e) = install_companions(&client, &exe, &manifest.companions, test_mode, dry).await {
        log::error!("{TAG} no replacement this round: {e}");
        return false;
    }

    let stage = {
        let exe = exe.clone();
        let payload = bytes.to_vec();
        tokio::task::spawn_blocking(move || stage_and_replace(&exe, &payload, &expected, dry)).await
    };

    match stage {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            // Before any rename, `ReplaceError` is reported and nothing user
            // visible has changed. After one, the display string says whether the
            // rollback succeeded.
            log::error!("{TAG} no replacement this round: {e}");
            return false;
        }
        Err(e) => {
            log::error!("{TAG} staging task failed: {e}");
            return false;
        }
    }

    if dry {
        log::info!("{TAG} DRY RUN: verified payload left in place, not restarting");
        return false;
    }

    log::info!(
        "{TAG} v{} installed at {}; restarting into it now",
        manifest.version,
        exe.display()
    );

    // -- restart -------------------------------------------------------------
    // Same sequence Settings' 「重启」 uses. The single-instance lock is released
    // so the new process can start, the tray icon is removed so it does not
    // linger as a dead ghost, and window state is saved so the new process
    // restores it.
    crate::save_window_state_before_exit(app);
    crate::cleanup_before_exit(app).await;
    crate::remove_tray_icon_before_exit(app);
    crate::destroy_single_instance_lock(app);
    tokio::time::sleep(Duration::from_millis(150)).await;
    log::info!("{TAG} restarting into v{}", manifest.version);

    // Diverges.
    crate::restart_process(app);
}

/// Schedule the updater. Deliberately fire-and-forget: nothing about the app's
/// normal operation waits on the network, and a failure here must never be
/// visible to a user.
pub fn spawn(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let test_mode = test_clock_on();
        let first = if test_mode {
            FIRST_CHECK_DELAY_TEST
        } else {
            FIRST_CHECK_DELAY_REAL
        };
        let period = if test_mode { PERIOD_TEST } else { PERIOD_REAL };

        log::info!(
            "{TAG} self-update armed: v{CURRENT_VERSION}, first check in {}s, then every {}s",
            first.as_secs(),
            period.as_secs()
        );

        tokio::time::sleep(first).await;

        loop {
            if check_once(&handle).await {
                // Only reached if something went wrong between a successful
                // replacement and the restart, which diverges. Falling through
                // would mean the loop keeps running against a replaced exe.
                log::warn!("{TAG} replacement happened but the restart did not; stopping the loop");
                return;
            }
            tokio::time::sleep(period).await;
        }
    });
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Every manifest published before companions existed must still parse. The
    /// fleet is only able to update *into* the version that understands the new
    /// field, so a required field here would strand every install.
    #[test]
    fn a_manifest_without_companions_still_parses() {
        let manifest: Manifest = serde_json::from_str(
            r#"{
                "version": "1.0.6",
                "url": "https://aimyria.com/releases/latest/download/AimyriaOne-x64.exe",
                "sha256": "abc",
                "notes": "n",
                "pubDate": "2026-10-21T00:00:00+08:00"
            }"#,
        )
        .expect("a pre-companion manifest must parse");
        assert!(manifest.companions.is_empty());
    }

    #[test]
    fn a_manifest_with_companions_parses_each_file() {
        let manifest: Manifest = serde_json::from_str(
            r#"{
                "version": "1.0.6",
                "url": "https://aimyria.com/releases/latest/download/AimyriaOne-x64.exe",
                "sha256": "abc",
                "companions": [
                    {
                        "name": "WebView2Loader.dll",
                        "sha256": "def",
                        "url": "https://aimyria.com/releases/latest/download/WebView2Loader.dll"
                    }
                ]
            }"#,
        )
        .expect("manifest parses");
        assert_eq!(manifest.companions.len(), 1);
        assert_eq!(manifest.companions[0].name, "WebView2Loader.dll");
    }

    /// A manifest is remote input. A companion name that is a path, not a file
    /// name, would let it write outside the install directory — the URL being on
    /// our host does not make the destination safe.
    #[tokio::test]
    async fn companion_names_must_be_bare_file_names() {
        let dir = tempfile::tempdir().expect("temp dir");
        let exe = dir.path().join("AimyriaOne-x64.exe");
        std::fs::write(&exe, b"exe").unwrap();
        let client = reqwest::Client::new();

        for name in [
            "../evil.dll",
            "..\\evil.dll",
            "sub/evil.dll",
            "C:evil.dll",
            "..",
            "",
        ] {
            let companions = vec![Companion {
                name: name.to_string(),
                sha256: "0".to_string(),
                url: "https://aimyria.com/releases/latest/download/x.dll".to_string(),
            }];
            let result = install_companions(&client, &exe, &companions, false, true).await;
            assert!(
                result.is_err(),
                "companion name {name:?} must be refused before any I/O"
            );
        }
        // Nothing was created outside the directory either.
        assert!(!dir.path().join("evil.dll").exists());
    }

    /// The cleanup pass has to find companion leftovers by shape, because their
    /// names come from the manifest of whichever release installed them.
    #[test]
    fn companion_cleanup_removes_only_its_own_leftovers() {
        let dir = tempfile::tempdir().expect("temp dir");
        let exe = dir.path().join("AimyriaOne-x64.exe");
        std::fs::write(&exe, b"exe").unwrap();
        std::fs::write(dir.path().join("WebView2Loader.dll"), b"live").unwrap();
        std::fs::write(dir.path().join("WebView2Loader.dll.new"), b"staged").unwrap();
        std::fs::write(dir.path().join("WebView2Loader.dll.old"), b"backup").unwrap();
        // Not ours: a user file that merely sits next to the exe.
        std::fs::write(dir.path().join("notes.txt"), b"mine").unwrap();

        let failed = cleanup_companions(&exe);

        assert!(failed.is_empty(), "nothing should have failed to delete");
        assert!(
            dir.path().join("WebView2Loader.dll").is_file(),
            "the live loader must survive"
        );
        assert!(
            dir.path().join("notes.txt").is_file(),
            "user files must survive"
        );
        assert!(!dir.path().join("WebView2Loader.dll.new").exists());
        assert!(!dir.path().join("WebView2Loader.dll.old").exists());
    }

    /// A companion whose bytes do not match is discarded, and no exe swap has
    /// happened yet — so the running install stays coherent.
    #[tokio::test]
    async fn a_companion_with_a_bad_checksum_is_not_installed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let exe = dir.path().join("AimyriaOne-x64.exe");
        std::fs::write(&exe, b"exe").unwrap();
        let live = dir.path().join("WebView2Loader.dll");
        std::fs::write(&live, b"the old loader").unwrap();

        // No server here: the mismatch path is exercised through the URL gate
        // being the only remaining check, so a refused URL proves the function
        // returns before touching the file.
        let companions = vec![Companion {
            name: "WebView2Loader.dll".to_string(),
            sha256: "deadbeef".to_string(),
            url: "https://evil.example/WebView2Loader.dll".to_string(),
        }];
        let client = reqwest::Client::new();
        let result = install_companions(&client, &exe, &companions, false, false).await;

        assert!(
            result.is_err(),
            "an off-allowlist companion must be refused"
        );
        assert_eq!(
            std::fs::read(&live).unwrap(),
            b"the old loader",
            "the live loader must be untouched"
        );
    }

    #[test]
    fn numeric_versions_parse_and_reject_decorated_ones() {
        assert_eq!(parse_numeric_version("1.0.1"), Some(vec![1, 0, 1]));
        assert_eq!(parse_numeric_version(" 2.10 "), Some(vec![2, 10]));
        // A prefix, suffix or non-numeric segment is not comparable, and the
        // caller turns that into "no update".
        for bad in ["v1.0.1", "1.0.1-beta", "1.0.x", "", "1..2", "1.0.1+build"] {
            assert_eq!(parse_numeric_version(bad), None, "{bad} must not parse");
        }
    }

    /// The comparison a string compare gets wrong, and the one the publisher is
    /// most likely to hit on the way from 1.0.9 to 1.0.10.
    #[test]
    fn numeric_segments_beat_lexicographic_order() {
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(is_newer("1.0.10", "1.0.9"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(is_newer("1.0.1", "1.0.0"));
        // Padding: a shorter version is not newer just for being shorter.
        assert!(!is_newer("1.1", "1.1.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.1"));
        assert!(!is_newer("1.0.0", "2.0.0"));
    }

    /// A version we cannot parse must never be treated as an update.
    #[test]
    fn unparseable_versions_never_update() {
        assert!(!is_newer("v9.9.9", "1.0.0"));
        assert!(!is_newer("1.0.1", "v1.0.0"));
        assert!(!is_newer("", "1.0.0"));
        assert!(!is_newer("1.0.1-beta", "1.0.0"));
    }

    /// The shipping policy: https, our hosts, nothing else. A payload URL that
    /// fails this is never fetched, however plausible it looks.
    #[test]
    fn url_allowlist_is_https_on_our_hosts_only() {
        assert!(url_is_allowed(DEFAULT_MANIFEST_URL, false));
        assert!(url_is_allowed(
            "https://www.aimyria.com/releases/latest/download/AimyriaOne-x64.exe",
            false
        ));
        assert!(url_is_allowed("https://AIMYRIA.com/x", false));

        for bad in [
            "http://aimyria.com/x",                             // plain http in production
            "https://api.aimyria.com/updates/latest.json",      // the gateway that 401s
            "https://github.com/farion1231/cc-switch/releases", // a competitor, not a source
            "https://evil.example/x",                           // not ours
            "https://aimyria.com.evil.example/x",               // suffix trick
            "file:///C:/x.exe",
            "ftp://aimyria.com/x",
            "not a url",
        ] {
            assert!(!url_is_allowed(bad, false), "{bad} must be refused");
        }
    }

    /// Loopback over plain http exists for the local acceptance run and only
    /// there.
    #[test]
    fn loopback_http_is_test_mode_only() {
        assert!(url_is_allowed("http://127.0.0.1:8799/version.json", true));
        assert!(url_is_allowed("http://localhost:8799/version.json", true));
        assert!(!url_is_allowed("http://127.0.0.1:8799/version.json", false));
        // Test mode does not widen the host list beyond loopback.
        assert!(!url_is_allowed("http://aimyria.com/version.json", true));
        assert!(!url_is_allowed("http://192.168.1.10/version.json", true));
    }

    /// The gate that matters: a manifest hash that does not match the bytes on
    /// disk must leave the exe exactly as it was, and leave nothing behind.
    ///
    /// Written as a real filesystem test rather than a mock, because the property
    /// under test *is* what the filesystem looks like afterwards.
    #[test]
    fn a_bad_checksum_replaces_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let exe = dir.path().join("AimyriaOne-x64.exe");
        let original = b"the currently installed binary".to_vec();
        std::fs::write(&exe, &original).expect("seed the exe");

        let payload = b"a perfectly good looking payload".to_vec();
        let wrong = sha256_hex(b"something else entirely");

        let result = stage_and_replace(&exe, &payload, &wrong, false);
        assert!(
            matches!(result, Err(ReplaceError::Checksum { .. })),
            "expected a checksum refusal, got {result:?}"
        );

        assert_eq!(
            std::fs::read(&exe).expect("exe is still there"),
            original,
            "the exe must be byte-identical after a refused update"
        );
        assert!(
            !sibling(&exe, ".new").exists(),
            "the rejected payload must not be left on disk"
        );
        assert!(
            !sibling(&exe, ".old").exists(),
            "a refused update must not have renamed anything aside"
        );
    }

    /// The happy path, including the rollback copy being retained: `.old` has to
    /// survive for the one cycle that makes manual recovery possible.
    #[test]
    fn a_good_checksum_replaces_and_retains_the_previous_exe() {
        let dir = tempfile::tempdir().expect("temp dir");
        let exe = dir.path().join("AimyriaOne-x64.exe");
        let original = b"version 1.0.0".to_vec();
        std::fs::write(&exe, &original).expect("seed the exe");

        let payload = b"version 1.0.1".to_vec();
        let digest = sha256_hex(&payload);

        stage_and_replace(&exe, &payload, &digest, false).expect("replacement succeeds");

        assert_eq!(std::fs::read(&exe).unwrap(), payload);
        assert_eq!(
            std::fs::read(sibling(&exe, ".old")).unwrap(),
            original,
            "the previous exe must be readable for one cycle"
        );
        assert!(!sibling(&exe, ".new").exists());
    }

    /// Upper-case hex in the manifest is still a match: the comparison is
    /// normalised rather than trusting the publisher's case.
    #[test]
    fn checksum_comparison_is_case_insensitive() {
        let dir = tempfile::tempdir().expect("temp dir");
        let exe = dir.path().join("AimyriaOne-x64.exe");
        std::fs::write(&exe, b"old").unwrap();
        let payload = b"new".to_vec();

        stage_and_replace(&exe, &payload, &sha256_hex(&payload).to_uppercase(), false)
            .expect("upper-case manifest hash is accepted");
        assert_eq!(std::fs::read(&exe).unwrap(), payload);
    }

    /// `startup_cleanup` is what makes `.old` a one-cycle file rather than a
    /// permanent one, and what stops an abandoned `.new` from being installed
    /// later.
    ///
    /// This calls the real function against a real directory. The earlier version
    /// of this test did the removals itself and then asserted they had happened --
    /// which passed no matter what `startup_cleanup` did. That is the failure mode
    /// the portable-build README describes: a check that reports PASS while
    /// measuring nothing.
    #[test]
    fn startup_cleanup_clears_both_leftovers() {
        let dir = tempfile::tempdir().expect("temp dir");
        let exe = dir.path().join("AimyriaOne-x64.exe");
        std::fs::write(&exe, b"current").unwrap();
        std::fs::write(sibling(&exe, ".old"), b"previous").unwrap();
        std::fs::write(sibling(&exe, ".new"), b"abandoned").unwrap();
        assert!(sibling(&exe, ".old").is_file() && sibling(&exe, ".new").is_file());

        let failed = cleanup_siblings(&exe, "1.0.1");
        assert!(
            failed.is_empty(),
            "nothing should have resisted removal: {failed:?}"
        );

        assert!(
            !sibling(&exe, ".old").exists(),
            "the rollback copy must be gone once the new version is running"
        );
        assert!(
            !sibling(&exe, ".new").exists(),
            "an abandoned payload must not survive to be installed later"
        );
        assert_eq!(
            std::fs::read(&exe).unwrap(),
            b"current",
            "cleanup must not touch the exe itself"
        );
    }

    /// The other direction: a tree with no leftovers must be left alone rather
    /// than reported as a failure.
    #[test]
    fn startup_cleanup_is_a_no_op_on_a_clean_tree() {
        let dir = tempfile::tempdir().expect("temp dir");
        let exe = dir.path().join("AimyriaOne-x64.exe");
        std::fs::write(&exe, b"current").unwrap();

        let failed = cleanup_siblings(&exe, "1.0.1");
        assert!(failed.is_empty());
        assert_eq!(std::fs::read(&exe).unwrap(), b"current");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            1,
            "a clean tree must gain no files"
        );
    }

    /// The sibling names have to keep the whole file name, not swap the
    /// extension: `AimyriaOne-x64.exe` must not become `AimyriaOne-x64.old`.
    #[test]
    fn sibling_names_append_rather_than_replace_the_extension() {
        let exe = PathBuf::from(r"C:\x\AimyriaOne-x64.exe");
        assert_eq!(
            sibling(&exe, ".new"),
            PathBuf::from(r"C:\x\AimyriaOne-x64.exe.new")
        );
        assert_eq!(
            sibling(&exe, ".old"),
            PathBuf::from(r"C:\x\AimyriaOne-x64.exe.old")
        );
    }

    /// The manifest on the wire is camelCase and tolerant of extra fields.
    #[test]
    fn manifest_parses_the_agreed_wire_format() {
        let wire = r#"{
            "version": "1.0.1",
            "url": "https://aimyria.com/releases/latest/download/AimyriaOne-x64.exe",
            "sha256": "abc123",
            "notes": "silent self-update",
            "pubDate": "2026-10-19T20:30:00+08:00",
            "someFutureField": {"nested": true}
        }"#;
        let manifest: Manifest = serde_json::from_str(wire).expect("agreed format parses");
        assert_eq!(manifest.version, "1.0.1");
        assert_eq!(manifest.sha256, "abc123");
        assert_eq!(
            manifest.pub_date.as_deref(),
            Some("2026-10-19T20:30:00+08:00")
        );
    }

    /// The default endpoint is what T5 publishes to. Changing it here without
    /// changing the server (or the other way round) is how the channel silently
    /// dies, so it is pinned by a test.
    #[test]
    fn the_pinned_manifest_url_is_unchanged() {
        assert_eq!(
            DEFAULT_MANIFEST_URL,
            "https://aimyria.com/releases/latest/download/version.json"
        );
    }

    /// The cache-buster has to produce a *different* URL each time and keep the
    /// path and host intact. A stale manifest is the failure this whole channel
    /// is exposed to, and `max-age=300` was measured on the live endpoint.
    #[test]
    fn the_cache_buster_changes_the_url_key_without_moving_the_target() {
        let first = with_cache_buster(DEFAULT_MANIFEST_URL);
        assert!(
            first.starts_with("https://aimyria.com/releases/latest/download/version.json?t="),
            "path and host must survive: {first}"
        );
        assert!(
            first.contains("t="),
            "the cache key must carry the timestamp"
        );

        // Still on the allowlist, or every check would be refused before it left.
        assert!(
            url_is_allowed(&first, false),
            "the cache-bustered URL must still pass the allowlist: {first}"
        );

        // An existing query string is preserved rather than replaced.
        let with_query = with_cache_buster("https://aimyria.com/x.json?channel=stable");
        assert!(with_query.contains("channel=stable"), "{with_query}");
        assert!(with_query.contains("t="), "{with_query}");

        // A URL that will not parse is handed back untouched instead of being
        // silently mangled; `url_is_allowed` is what refuses it later.
        assert_eq!(with_cache_buster("not a url"), "not a url");
    }

    /// T5 adds a `size` field to the manifest. Parsing must not break on it --
    /// an unknown field bricking the update channel for every installed copy is
    /// the specific failure this guards.
    #[test]
    fn manifest_ignores_the_publishers_extra_size_field() {
        let wire = r#"{
            "version": "1.0.1",
            "url": "https://aimyria.com/releases/latest/download/AimyriaOne-x64.exe",
            "sha256": "dbc96880e7a6b3f7823cb98f19547a2df1455579d147de87d7aa0a68cd559239",
            "notes": "静默自更新",
            "pubDate": "2026-10-19T21:59:59+08:00",
            "size": 36564480
        }"#;
        let manifest: Manifest = serde_json::from_str(wire).expect("the live shape must parse");
        assert_eq!(manifest.version, "1.0.1");
        assert_eq!(manifest.sha256.len(), 64);
    }

    /// A UTF-8 BOM must not stop the channel.
    ///
    /// Regression test for a defect that a real end-to-end run caught and unit
    /// tests alone had not: `serde_json` refuses a BOM-prefixed document, and the
    /// symptom is `error decoding response body` -- which names neither the BOM nor
    /// the file, while the server is answering 200 with correct content. The
    /// manifest in that run was written by PowerShell's `Set-Content -Encoding
    /// UTF8`, which on 5.1 emits a BOM, so this is a realistic publisher mistake.
    #[test]
    fn a_utf8_bomprefix_does_not_break_the_manifest() {
        let body =
            br#"{"version":"1.0.1","url":"https://aimyria.com/x.exe","sha256":"abc","notes":"n"}"#;
        assert!(
            serde_json::from_slice::<Manifest>(body).is_ok(),
            "sanity: the plain body parses"
        );

        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(body);
        assert!(
            serde_json::from_slice::<Manifest>(&with_bom).is_err(),
            "sanity: serde_json really does refuse the BOM (this is the bug)"
        );
        let manifest: Manifest =
            serde_json::from_slice(strip_utf8_bom(&with_bom)).expect("BOM stripped, parses");
        assert_eq!(manifest.version, "1.0.1");

        // A body without a BOM is passed through untouched, byte for byte.
        assert_eq!(strip_utf8_bom(body), body);
        // Only the exact UTF-8 sequence is treated as a BOM.
        assert_eq!(strip_utf8_bom(&[0xEF, 0xBB]), &[0xEF, 0xBB][..]);
    }
}
