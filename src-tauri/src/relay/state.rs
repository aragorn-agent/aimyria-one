//! The in-memory half of the session: what the process knows right now.
//!
//! # Why the access token is not on disk
//!
//! [`super::session`] persists the *refresh* credential because it must survive a
//! restart. The access token deliberately lives only in memory: it is short-lived,
//! it is replaceable in one request, and writing it to disk would double the number
//! of copies of a live credential for no benefit.
//!
//! # Why refreshing is single-flight
//!
//! On startup the app can easily fire five console requests at once (account,
//! balance, keys, models, usage). If each one discovered an expired access token
//! and refreshed independently, the server would see five refreshes for one
//! session and — under the rotation model in `docs/relay-integration-plan.md` §0.1
//! — four of them would be presenting a credential that the first one already
//! rotated away. The result is a spurious "signed out" on a perfectly good session.
//!
//! So: one refresh at a time, everyone else waits for its result. [`SessionState`]
//! owns that gate.
//!
//! # Why the token is behind a channel-free lock
//!
//! `tokio::sync::Mutex` is used rather than `std::sync::Mutex` because the holder
//! awaits a network call. Holding a std mutex across `.await` is how a UI freezes.

use std::sync::Arc;

use tokio::sync::Mutex as AsyncMutex;

use super::session::{self, StoredSession};

/// The live session, as the process currently knows it.
pub struct SessionState {
    /// What was loaded from (or last written to) disk.
    stored: AsyncMutex<Option<StoredSession>>,
    /// Short-lived access token. `None` means "must refresh before use".
    access: AsyncMutex<Option<AccessToken>>,
    /// Serialises refreshes. See the module docs.
    refresh_gate: AsyncMutex<()>,
    /// Has this session been confirmed against the server in this process?
    ///
    /// False for a session that came off disk at startup, true once a refresh or a
    /// login has succeeded. See [`SessionStatus::unverified`] for why this matters.
    verified: std::sync::atomic::AtomicBool,
}

#[derive(Debug, Clone)]
pub struct AccessToken {
    pub value: String,
    /// Unix seconds. `None` = the server said it does not expire.
    pub expires_at: Option<i64>,
}

impl AccessToken {
    /// Is this token usable at `now`?
    ///
    /// A 60-second skew is deliberate: a token that expires while the request is
    /// in flight produces a 401 the caller then has to handle, and it is cheaper to
    /// refresh slightly early than to eat that round trip on every request near the
    /// boundary.
    pub fn is_fresh(&self, now: i64) -> bool {
        match self.expires_at {
            None => true,
            Some(at) => at - 60 > now,
        }
    }
}

/// What the UI needs to render the account bar.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    /// Is there a usable session (possibly needing a silent refresh)?
    pub signed_in: bool,
    /// Account identifier, for display. Empty when signed out.
    pub account: String,
    /// When the current refresh credential lapses, if it does. The UI uses this
    /// only for diagnostics; the user is never shown a countdown.
    pub refresh_expires_at: Option<i64>,
    /// True when the stored session exists but could not be refreshed — the
    /// caller should show the login form *with an explanation* rather than
    /// silently pretending the user was never signed in.
    pub needs_reauth: bool,
    /// The stored session exists but has never been confirmed against the server
    /// in this install.
    ///
    /// A `session.bin` can appear without the app ever having logged in: a file
    /// restored from a backup, copied from another profile, left behind by a test
    /// build pointed at a mock server, or written by something else entirely. The
    /// old code trusted it and showed "已登录 user@example.com" — a state the user
    /// could not act on and the app could not satisfy, because every request then
    /// failed.
    ///
    /// So the file alone is a *claim*, not proof. A session is only reported as
    /// signed in once a request has actually succeeded with it, or once it was
    /// created by a login in this process. Until then the account bar says
    /// "需要验证" and the page retries the refresh path, which either promotes it to
    /// verified or clears it.
    pub unverified: bool,
}

impl SessionStatus {
    fn signed_out() -> Self {
        Self {
            signed_in: false,
            account: String::new(),
            refresh_expires_at: None,
            needs_reauth: false,
            unverified: false,
        }
    }
}

impl SessionState {
    /// Build from whatever is on disk, without doing any network I/O.
    ///
    /// Called once during startup. A corrupt or absent file is not an error: the
    /// app starts signed out, which is a normal state.
    ///
    /// A session found on disk starts **unverified**: the file is a claim, not
    /// proof, and only a successful refresh (or a login in this process) can
    /// promote it. See [`SessionStatus::unverified`].
    pub fn load() -> Self {
        let stored = session::load();
        let has_stored = stored.is_some();
        Self {
            stored: AsyncMutex::new(stored),
            access: AsyncMutex::new(None),
            refresh_gate: AsyncMutex::new(()),
            // A file on disk is not evidence the server still honours it.
            verified: std::sync::atomic::AtomicBool::new(!has_stored),
        }
    }

    /// Build with an explicit stored session (tests).
    ///
    /// `verified` follows the same rule as [`Self::load`]: a pre-seeded session is
    /// unverified until something proves it.
    pub fn with_session(stored: Option<StoredSession>) -> Self {
        let has_stored = stored.is_some();
        Self {
            stored: AsyncMutex::new(stored),
            access: AsyncMutex::new(None),
            refresh_gate: AsyncMutex::new(()),
            verified: std::sync::atomic::AtomicBool::new(!has_stored),
        }
    }

    /// Mark the session as confirmed against the server.
    ///
    /// Called after a successful refresh or login. Only then may the account bar
    /// say the user is signed in.
    pub fn mark_verified(&self) {
        self.verified
            .store(true, std::sync::atomic::Ordering::Release);
    }

    fn is_verified(&self) -> bool {
        self.verified.load(std::sync::atomic::Ordering::Acquire)
    }

    pub async fn status(&self) -> SessionStatus {
        let stored = self.stored.lock().await;
        match stored.as_ref() {
            None => SessionStatus::signed_out(),
            Some(session) => {
                let verified = self.is_verified();
                SessionStatus {
                    // A stored-but-unproven session must NOT report as signed in,
                    // or the UI shows an account the app cannot actually act for.
                    // The file is still kept so the first refresh can promote it
                    // without asking the user for a password.
                    signed_in: verified,
                    account: session.account.clone(),
                    refresh_expires_at: session.expires_at,
                    needs_reauth: false,
                    unverified: !verified,
                }
            }
        }
    }

    /// The refresh credential, if any.
    pub async fn refresh_token(&self) -> Option<String> {
        self.stored
            .lock()
            .await
            .as_ref()
            .map(|session| session.refresh_token.clone())
    }

    /// The account name stored alongside the credential, if any.
    ///
    /// Separate from [`Self::status`] because callers that just need the display
    /// name should not have to reason about whether the session is verified yet.
    pub async fn account(&self) -> Option<String> {
        self.stored
            .lock()
            .await
            .as_ref()
            .map(|session| session.account.clone())
    }

    /// A currently-valid access token, or `None`.
    pub async fn access_token(&self, now: i64) -> Option<String> {
        self.access
            .lock()
            .await
            .as_ref()
            .filter(|token| token.is_fresh(now))
            .map(|token| token.value.clone())
    }

    /// Install a freshly minted access token.
    pub async fn set_access_token(&self, value: String, expires_at: Option<i64>) {
        *self.access.lock().await = Some(AccessToken { value, expires_at });
    }

    /// Replace the stored session (after login or a refresh that rotated the
    /// refresh credential).
    ///
    /// Persists before returning. A failure to write is reported to the caller but
    /// does **not** roll back the in-memory state: the user is legitimately signed
    /// in for this run, and turning that into a hard error would log them out over
    /// a disk problem. The next launch will ask them to sign in again, which is the
    /// honest degradation.
    pub async fn set_session(&self, session: StoredSession) -> Result<(), String> {
        if let Err(error) = super::session::save(&session) {
            log::warn!("会话已建立但未能写入磁盘（下次启动需重新登录）: {error}");
            *self.stored.lock().await = Some(session);
            return Err(error.to_string());
        }
        *self.stored.lock().await = Some(session);
        Ok(())
    }

    /// Drop the session everywhere: memory, disk, and the access token.
    ///
    /// This is the *only* path that ends a session, per the product requirement.
    pub async fn clear(&self) {
        *self.access.lock().await = None;
        *self.stored.lock().await = None;
        session::clear();
    }

    /// Take the refresh gate.
    ///
    /// Callers must hold the returned guard across the refresh request, then call
    /// [`Self::access_token`] again to see whether somebody else already refreshed
    /// while they waited. That double-check is what makes the gate work: the
    /// waiting task must not refresh a second time just because it queued.
    pub async fn begin_refresh(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.refresh_gate.lock().await
    }
}

/// Shared handle stored in `AppState`.
pub type SharedSession = Arc<SessionState>;

/// Convenience constructor used at startup.
pub fn shared() -> SharedSession {
    Arc::new(SessionState::load())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(expires_at: Option<i64>) -> StoredSession {
        StoredSession {
            refresh_token: "rt-test".to_string(),
            account: "test@example.com".to_string(),
            expires_at,
        }
    }

    #[tokio::test]
    async fn a_fresh_process_with_no_file_is_signed_out() {
        let state = SessionState::with_session(None);
        let status = state.status().await;
        assert!(!status.signed_in);
        assert!(!status.needs_reauth, "absence is not a re-auth demand");
        assert_eq!(state.refresh_token().await, None);
    }

    #[tokio::test]
    async fn a_stored_session_reports_the_account_without_any_network() {
        let state = SessionState::with_session(Some(sample(Some(1_900_000_000))));
        let status = state.status().await;
        // The account name is readable without a round trip...
        assert_eq!(status.account, "test@example.com");
        assert_eq!(state.refresh_token().await.as_deref(), Some("rt-test"));
        // ...but "signed in" waits for the server to honour the credential.
        assert!(!status.signed_in);
        assert!(status.unverified);
    }

    #[tokio::test]
    async fn an_access_token_expires_with_skew() {
        let token = AccessToken {
            value: "at".into(),
            expires_at: Some(1000),
        };
        assert!(token.is_fresh(900), "well before expiry");
        assert!(
            !token.is_fresh(950),
            "inside the 60s skew, treat as stale so the request does not 401"
        );
        assert!(!token.is_fresh(1000), "at expiry");
    }

    #[tokio::test]
    async fn a_token_without_expiry_is_always_fresh() {
        let token = AccessToken {
            value: "at".into(),
            expires_at: None,
        };
        assert!(token.is_fresh(i64::MAX));
    }

    #[tokio::test]
    async fn a_stored_session_is_not_trusted_until_the_server_confirms_it() {
        let state = SessionState::with_session(Some(sample(Some(1_900_000_000))));
        let status = state.status().await;

        assert!(
            !status.signed_in,
            "a file on disk is a claim, not proof; the app must not show an account \
             it has never been able to act for"
        );
        assert!(status.unverified, "the UI needs to know to retry");
        // The account name is still available so a retry can be silent.
        assert_eq!(status.account, "test@example.com");
        assert_eq!(state.refresh_token().await.as_deref(), Some("rt-test"));

        state.mark_verified();
        let status = state.status().await;
        assert!(status.signed_in, "after a successful refresh, it is signed in");
        assert!(!status.unverified);
    }

    /// A real login marks the session verified by construction.
    #[tokio::test]
    async fn marking_verified_survives_a_status_read() {
        let state = SessionState::with_session(Some(sample(None)));
        state.mark_verified();
        assert!(state.status().await.signed_in);
        assert!(!state.status().await.unverified);
    }

    #[tokio::test]
    async fn clearing_removes_both_halves() {
        let state = SessionState::with_session(Some(sample(None)));
        state.set_access_token("at".into(), None).await;
        assert!(state.access_token(0).await.is_some());

        state.clear().await;

        assert_eq!(state.refresh_token().await, None);
        assert_eq!(state.access_token(0).await, None);
        assert!(!state.status().await.signed_in);
    }

    /// Two concurrent callers must not both enter the refresh critical section.
    ///
    /// This is the property that keeps a startup burst of five requests from
    /// rotating the refresh credential five times and signing the user out.
    #[tokio::test]
    async fn the_refresh_gate_admits_one_caller_at_a_time() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let state = Arc::new(SessionState::with_session(Some(sample(None))));
        let concurrent = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let state = state.clone();
            let concurrent = concurrent.clone();
            let peak = peak.clone();
            handles.push(tokio::spawn(async move {
                let _guard = state.begin_refresh().await;
                let now = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                concurrent.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(
            peak.load(Ordering::SeqCst),
            1,
            "no two refreshes may overlap; overlap rotates the credential twice"
        );
    }
}
