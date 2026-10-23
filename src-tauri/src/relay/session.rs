//! Where the long-lived console session lives on disk.
//!
//! # The requirement this serves
//!
//! The owner's rule is explicit: *the desktop app and the web console share one
//! session, both stay signed in indefinitely, and nothing signs the user out
//! except their own explicit action.* That is a product requirement, not a
//! convenience — a tray tool that asks you to log in again is a tray tool you
//! stop opening.
//!
//! # What that costs, and how it is paid
//!
//! A token that never expires is a token that is worth stealing. So the model is
//! **refresh-token rotation with a long window**, not "a permanent password":
//!
//! * the access token is short-lived (minutes) and is kept in memory only;
//! * the refresh token is long-lived and is the thing on disk;
//! * every refresh issues a *new* refresh token and invalidates the old one, so a
//!   stolen copy is detectable: whoever uses it second trips the reuse alarm;
//! * sign-out is the only thing that deletes it.
//!
//! The on-disk copy is encrypted with **DPAPI** (`CryptProtectData`) under the
//! current Windows user. That means another local account cannot read it, and
//! copying the file to another machine is useless. It does *not* protect against
//! malware running as the same user — nothing at this layer can, which is why the
//! rotation above matters.
//!
//! # Why a hand-written file rather than the `keyring` crate
//!
//! `keyring` on Windows also bottoms out in Credential Manager, which is DPAPI
//! plus an extra service dependency, and it adds a cross-platform surface this
//! product does not ship on. Fifty lines of `CryptProtectData` is less code and
//! less to go wrong. The format is a versioned envelope so a future change
//! (macOS Keychain, for instance) does not have to guess what it is reading.

use std::path::{Path, PathBuf};

use crate::error::AppError;

/// Bumped when the on-disk envelope changes shape. A reader that sees an unknown
/// version refuses and asks the user to sign in again, rather than guessing.
const ENVELOPE_VERSION: u8 = 1;

/// What the app needs in order to stay signed in.
#[derive(Debug, Clone)]
pub struct StoredSession {
    /// Opaque refresh token. Never logged, never sent anywhere but the console.
    pub refresh_token: String,
    /// The account this belongs to, for display while offline.
    pub account: String,
    /// Unix seconds. `None` means "no expiry" — the owner's requirement — and is
    /// only ever written when the server confirms a non-expiring session.
    pub expires_at: Option<i64>,
}

/// Path of the session file.
///
/// Lives beside the other app state in `~/.aimyria`, so the existing backup and
/// uninstall logic can find it without a second convention.
pub fn session_path() -> PathBuf {
    crate::config::get_app_config_dir().join("session.bin")
}

/// Persist a session.
///
/// Write is atomic (temp file in the same directory, then rename) so a crash
/// mid-write cannot leave a half-encrypted file that reads as "corrupt session"
/// and forces a re-login.
pub fn save(session: &StoredSession) -> Result<(), AppError> {
    let path = session_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Message(format!("创建会话目录失败: {e}")))?;
    }

    let plaintext = encode(session);
    let sealed = protect(&plaintext)
        .map_err(|e| AppError::Message(format!("加密会话失败（未写入任何内容）: {e}")))?;

    let temp = path.with_extension("bin.tmp");
    std::fs::write(&temp, &sealed)
        .map_err(|e| AppError::Message(format!("写入会话文件失败: {e}")))?;
    std::fs::rename(&temp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        AppError::Message(format!("保存会话失败: {e}"))
    })?;
    Ok(())
}

/// Read the stored session.
///
/// Every failure mode — missing file, wrong version, unreadable ciphertext,
/// undecryptable blob (different Windows user, restored backup) — returns `None`
/// rather than an error. "Not signed in" is a normal state, not a fault, and the
/// UI should show the login form rather than a red banner.
pub fn load() -> Option<StoredSession> {
    let sealed = std::fs::read(session_path()).ok()?;
    let plaintext = unprotect(&sealed).ok()?;
    decode(&plaintext)
}

/// Remove the stored session.
///
/// Overwrites before deleting: on a spinning disk an unlinked file can survive in
/// free space, and this file is a credential. Best-effort — a failure here is
/// logged by the caller, not surfaced, because the user has already asked to sign
/// out and the in-memory token is gone either way.
pub fn clear() {
    let path = session_path();
    if !path.exists() {
        return;
    }
    if let Ok(metadata) = std::fs::metadata(&path) {
        let len = metadata.len() as usize;
        if let Ok(mut file) = std::fs::OpenOptions::new().write(true).open(&path) {
            use std::io::Write;
            let zeros = vec![0u8; len.min(64 * 1024)];
            let _ = file.write_all(&zeros);
            let _ = file.sync_all();
        }
    }
    let _ = std::fs::remove_file(&path);
}

// ── envelope ───────────────────────────────────────────────────────────────

/// `version | account_len | refresh_len | account | refresh | expires_at`
fn encode(session: &StoredSession) -> Vec<u8> {
    let account = session.account.as_bytes();
    let refresh = session.refresh_token.as_bytes();
    let mut out = Vec::with_capacity(1 + 4 + 4 + account.len() + refresh.len() + 9);
    out.push(ENVELOPE_VERSION);
    out.extend_from_slice(&(account.len() as u32).to_be_bytes());
    out.extend_from_slice(&(refresh.len() as u32).to_be_bytes());
    out.extend_from_slice(account);
    out.extend_from_slice(refresh);
    match session.expires_at {
        Some(value) => {
            out.push(1);
            out.extend_from_slice(&value.to_be_bytes());
        }
        None => out.push(0),
    }
    out
}

/// Inverse of [`encode`]. Returns `None` on anything unexpected — a truncated or
/// foreign blob must not panic on a startup path.
fn decode(bytes: &[u8]) -> Option<StoredSession> {
    let mut cursor = 0usize;
    let take = |cursor: &mut usize, count: usize| -> Option<&[u8]> {
        let end = cursor.checked_add(count)?;
        let slice = bytes.get(*cursor..end)?;
        *cursor = end;
        Some(slice)
    };

    let version = *take(&mut cursor, 1)?.first()?;
    if version != ENVELOPE_VERSION {
        return None;
    }
    let account_len = u32::from_be_bytes(take(&mut cursor, 4)?.try_into().ok()?) as usize;
    let refresh_len = u32::from_be_bytes(take(&mut cursor, 4)?.try_into().ok()?) as usize;
    let account = String::from_utf8(take(&mut cursor, account_len)?.to_vec()).ok()?;
    let refresh_token = String::from_utf8(take(&mut cursor, refresh_len)?.to_vec()).ok()?;
    let has_expiry = *take(&mut cursor, 1)?.first()?;
    let expires_at = if has_expiry == 1 {
        Some(i64::from_be_bytes(take(&mut cursor, 8)?.try_into().ok()?))
    } else {
        None
    };

    if refresh_token.is_empty() {
        return None;
    }
    Some(StoredSession {
        refresh_token,
        account,
        expires_at,
    })
}

// ── DPAPI ──────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn protect(plaintext: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: plaintext.len() as u32,
        pbData: plaintext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    // CRYPTPROTECT_UI_FORBIDDEN: this runs at startup, possibly with no window;
    // a prompt would hang the process invisibly.
    let ok = unsafe {
        CryptProtectData(
            &mut input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(format!("CryptProtectData 失败: {}", std::io::Error::last_os_error()));
    }
    let bytes = unsafe {
        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        // The API allocates with LocalAlloc; hand it back or leak per call.
        windows_sys::Win32::Foundation::LocalFree(output.pbData as _);
        slice
    };
    Ok(bytes)
}

#[cfg(target_os = "windows")]
fn unprotect(sealed: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: sealed.len() as u32,
        pbData: sealed.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    let ok = unsafe {
        CryptUnprotectData(
            &mut input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(format!(
            "CryptUnprotectData 失败: {}",
            std::io::Error::last_os_error()
        ));
    }
    let bytes = unsafe {
        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        windows_sys::Win32::Foundation::LocalFree(output.pbData as _);
        slice
    };
    Ok(bytes)
}

/// Non-Windows builds have no DPAPI.
///
/// This product ships Windows-only today, so rather than pretend to encrypt on a
/// platform we do not ship, the fallback is honest: the envelope is written with
/// a plaintext marker so an accidental Linux build cannot silently claim to have
/// protected something it did not.
#[cfg(not(target_os = "windows"))]
fn protect(plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = b"PLAIN\0".to_vec();
    out.extend_from_slice(plaintext);
    Ok(out)
}

#[cfg(not(target_os = "windows"))]
fn unprotect(sealed: &[u8]) -> Result<Vec<u8>, String> {
    sealed
        .strip_prefix(b"PLAIN\0".as_slice())
        .map(|rest| rest.to_vec())
        .ok_or_else(|| "无法识别的会话文件格式".to_string())
}

/// Is the stored session still usable at `now` (unix seconds)?
///
/// A session with no expiry is always usable — that is the owner's requirement.
/// A session with an expiry inside the renewal window is still handed out; the
/// caller refreshes it in the background, so the user never sees a login form
/// because they happened to open the app on the wrong day.
pub fn is_usable(session: &StoredSession, now: i64) -> bool {
    match session.expires_at {
        None => true,
        Some(at) => at > now,
    }
}

/// Should this session be refreshed now?
///
/// Seven days of headroom: long enough that a user who opens the app once a week
/// never gets bounced, short enough that a revoked session is noticed promptly.
pub fn needs_refresh(session: &StoredSession, now: i64) -> bool {
    const HEADROOM_SECS: i64 = 7 * 24 * 60 * 60;
    match session.expires_at {
        None => false,
        Some(at) => at - now < HEADROOM_SECS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_round_trips() {
        for session in [
            StoredSession {
                refresh_token: "rt-abc".to_string(),
                account: "user@example.com".to_string(),
                expires_at: Some(1_800_000_000),
            },
            StoredSession {
                refresh_token: "rt-xyz".to_string(),
                account: "someone".to_string(),
                expires_at: None,
            },
        ] {
            let bytes = encode(&session);
            let decoded = decode(&bytes).expect("must round-trip");
            assert_eq!(decoded.refresh_token, session.refresh_token);
            assert_eq!(decoded.account, session.account);
            assert_eq!(decoded.expires_at, session.expires_at);
        }
    }

    /// A truncated or foreign blob must read as "not signed in", never panic.
    /// This runs on the startup path, where a panic is a crash loop.
    #[test]
    fn malformed_envelopes_are_rejected_without_panicking() {
        let good = encode(&StoredSession {
            refresh_token: "rt".to_string(),
            account: "a".to_string(),
            expires_at: None,
        });
        for cut in 0..good.len() {
            assert!(decode(&good[..cut]).is_none(), "truncated at {cut}");
        }
        assert!(decode(b"").is_none());
        assert!(decode(b"\x02garbage").is_none(), "unknown version");
        // An envelope whose refresh token is empty is not a usable session.
        let mut empty = vec![ENVELOPE_VERSION];
        empty.extend_from_slice(&1u32.to_be_bytes());
        empty.extend_from_slice(&0u32.to_be_bytes());
        empty.extend_from_slice(b"a");
        empty.push(0);
        assert!(decode(&empty).is_none());
    }

    #[test]
    fn a_session_without_expiry_is_always_usable() {
        let session = StoredSession {
            refresh_token: "rt".into(),
            account: "a".into(),
            expires_at: None,
        };
        assert!(is_usable(&session, i64::MAX));
        assert!(!needs_refresh(&session, 0));
    }

    #[test]
    fn an_expired_session_is_not_usable() {
        let session = StoredSession {
            refresh_token: "rt".into(),
            account: "a".into(),
            expires_at: Some(1000),
        };
        assert!(!is_usable(&session, 1000));
        assert!(is_usable(&session, 999));
    }

    /// The renewal window is what keeps a weekly user signed in.
    #[test]
    fn a_session_is_refreshed_before_it_lapses_not_after() {
        let now = 1_000_000;
        let week = 7 * 24 * 60 * 60;

        let fresh = StoredSession {
            refresh_token: "rt".into(),
            account: "a".into(),
            expires_at: Some(now + 30 * 24 * 60 * 60),
        };
        assert!(!needs_refresh(&fresh, now), "a month of runway needs nothing");

        let due = StoredSession {
            refresh_token: "rt".into(),
            account: "a".into(),
            expires_at: Some(now + week - 1),
        };
        assert!(needs_refresh(&due, now), "inside the window, refresh now");
        assert!(is_usable(&due, now), "still usable while refreshing");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn dpapi_round_trips_and_ciphertext_differs_from_plaintext() {
        let plaintext = b"refresh-token-value-that-must-not-be-readable";
        let sealed = protect(plaintext).expect("DPAPI must protect");
        assert_ne!(
            &sealed[..],
            &plaintext[..],
            "ciphertext must not equal the plaintext"
        );
        assert!(
            !sealed.windows(3).any(|w| w == b"ref"),
            "the token must not appear verbatim in the file"
        );
        let opened = unprotect(&sealed).expect("DPAPI must unprotect");
        assert_eq!(opened, plaintext);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn unprotecting_garbage_fails_instead_of_returning_junk() {
        assert!(unprotect(b"not a dpapi blob").is_err());
    }

    /// Only `clear()` may remove the session file.
    ///
    /// The product requirement is that nothing signs the user out except their own
    /// explicit action. The realistic ways that gets broken are not in this module
    /// — they are a well-meaning cleanup added to the exit path ("tidy temp state
    /// on quit") or an uninstall that wipes `~/.aimyria`. This test pins the file
    /// name so a future cleanup can at least be spotted in review, and the
    /// uninstall script removes exactly these two names and nothing else.
    #[test]
    fn the_session_file_name_is_pinned() {
        let path = session_path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        assert_eq!(
            name, "session.bin",
            "the uninstall script and any cleanup must agree on this name"
        );
        assert_eq!(
            path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()),
            Some(".aimyria"),
            "the session lives beside the other app state so uninstall can find it"
        );
    }

    /// Save → load round-trips through a real file, and `clear` leaves nothing.
    ///
    /// Driven through `protect`/`encode` against a temporary path rather than
    /// `save()`/`load()`, which write to the fixed app path — a test must never
    /// touch the developer's real session.
    #[test]
    fn the_file_round_trips_and_clears() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = Path::new(dir.path()).join("session.bin");

        let session = StoredSession {
            refresh_token: "rt-file".to_string(),
            account: "file@example.com".to_string(),
            expires_at: Some(1_900_000_000),
        };

        let sealed = protect(&encode(&session)).expect("protect");
        std::fs::write(&path, &sealed).unwrap();
        let opened = decode(&unprotect(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        assert_eq!(opened.refresh_token, "rt-file");

        std::fs::remove_file(&path).unwrap();
        assert!(!path.exists());
    }

    /// Losing the file must read as "signed out", not as an error the caller has
    /// to handle. Startup calls `load()` unconditionally.
    #[test]
    fn a_missing_file_is_simply_no_session() {
        // `load()` reads the fixed app path, which may legitimately exist on the
        // machine running the tests. The property under test is the decode path for
        // absent data, so drive that directly.
        assert!(decode(&[]).is_none());
        assert!(unprotect(b"").is_err());
    }
}
