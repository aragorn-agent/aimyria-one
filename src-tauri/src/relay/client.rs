//! One request path for every console call.
//!
//! # What "transparent refresh" means here
//!
//! A caller asks for `/desktop/v1/keys` and gets JSON. It never sees an access
//! token, never handles a 401, and never retries. This module:
//!
//! 1. attaches the current access token (refreshing first if it is stale);
//! 2. sends the request;
//! 3. on 401, refreshes **once** under the single-flight gate, then replays the
//!    request exactly once;
//! 4. if the replay also 401s, the refresh credential is genuinely dead — the
//!    session is cleared and `NeedsReauth` is returned.
//!
//! Retrying exactly once is deliberate. A loop would turn a server-side auth bug
//! into an infinite request storm, and it would hide the very signal the user
//! needs ("your session ended, sign in again").
//!
//! # The failure taxonomy
//!
//! Callers get [`RelayError`], not `String`, because the four cases need four
//! different UI treatments and every one of them has been confused with another
//! at some point:
//!
//! | variant | what the user sees |
//! |---|---|
//! | `Offline` | "网络不可用，显示的是缓存数据" — no login prompt |
//! | `NeedsReauth` | login form, with "会话已结束" |
//! | `Rejected` | the server's own message, verbatim |
//! | `Server` | "中转站暂时不可用，请稍后重试" + retry affordance |
//!
//! Collapsing these into one error string is how "the relay is down" becomes "you
//! are logged out", which is the single most common complaint about tools in this
//! category.

use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use super::hosts::{is_relay_endpoint, RelayHost};
use super::session::StoredSession;
use super::state::SharedSession;
use super::{Credential, Endpoint};

/// Console base URL.
///
/// The console's account API lives under `/api/v1/*` and authenticates with the
/// **same JWT the website uses** — it is not a desktop-specific surface. Measured:
/// `POST /api/v1/auth/login` answers 200 with `access_token` + `refresh_token`, and
/// `GET /api/v1/keys` returns the account's keys. An earlier design assumed a
/// `/desktop/v1/*` namespace that was never built, which is why the login form in
/// the first attempt could only ever print 404.
const CONSOLE_BASE: &str = "https://aimyria.com";

/// Path prefix every account call shares.
const API_PREFIX: &str = "/api/v1";

/// Where a browser-facing login/pay page lives, for the "open the site" actions.
pub const CONSOLE_SITE: &str = "https://aimyria.com";

#[derive(Debug)]
pub enum RelayError {
    /// Transport failed. The session is untouched — this is not an auth problem.
    Offline(String),
    /// The server refused the credential and a refresh could not fix it.
    NeedsReauth,
    /// The server answered 4xx with a message meant for a human.
    Rejected { status: u16, message: String },
    /// 5xx, or a body we could not parse. Retryable, not an auth problem.
    Server(String),
}

impl RelayError {
    /// Machine-readable code for the frontend, so it can branch without parsing text.
    pub fn code(&self) -> &'static str {
        match self {
            RelayError::Offline(_) => "offline",
            RelayError::NeedsReauth => "needs_reauth",
            RelayError::Rejected { .. } => "rejected",
            RelayError::Server(_) => "server_error",
        }
    }

    /// A sentence a user can act on.
    pub fn user_message(&self) -> String {
        match self {
            RelayError::Offline(detail) => {
                format!("网络不可用，暂时无法连接中转站（{detail}）")
            }
            RelayError::NeedsReauth => "登录已结束，请重新登录".to_string(),
            RelayError::Rejected { message, .. } if !message.is_empty() => message.clone(),
            RelayError::Rejected { status, .. } => format!("请求被拒绝（HTTP {status}）"),
            RelayError::Server(detail) => format!("中转站暂时不可用，请稍后重试（{detail}）"),
        }
    }

    /// Should the UI show the login form?
    pub fn requires_login(&self) -> bool {
        matches!(self, RelayError::NeedsReauth)
    }
}

impl std::fmt::Display for RelayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.user_message())
    }
}

/// A console client bound to one session.
#[derive(Clone)]
pub struct RelayClient {
    http: reqwest::Client,
    session: SharedSession,
    base: String,
}

/// Server response to a successful refresh.
/// The console wraps every response in `{code, message, data}`.
///
/// `code` is 0 on success; a non-zero code carries a human message. This is the
/// outermost parse: everything below it sees only `data`, so a change to the
/// envelope happens in one place.
#[derive(Debug, Deserialize)]
struct Envelope<T> {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
    data: Option<T>,
}

/// Token pair the console issues.
#[derive(Debug, Deserialize)]
struct TokenPair {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    /// Seconds, not an absolute timestamp. Converted to one on arrival.
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    refresh_expires_in: Option<i64>,
}

impl RelayClient {
    pub fn new(session: SharedSession) -> Self {
        Self::with_base(session, CONSOLE_BASE)
    }

    /// Explicit base, for tests and for a future self-hosted console.
    pub fn with_base(session: SharedSession, base: impl Into<String>) -> Self {
        Self {
            // Reuse the shared pool: a console request must not open its own TLS
            // connection while the rest of the app shares one.
            http: crate::proxy::http_client::get(),
            session,
            base: base.into().trim_end_matches('/').to_string(),
        }
    }

    pub fn session(&self) -> &SharedSession {
        &self.session
    }

    /// GET a console path and parse the JSON body.
    pub async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
    ) -> Result<T, RelayError> {
        let value = self.request(reqwest::Method::GET, path, None::<&()>).await?;
        serde_json::from_value(value).map_err(|e| {
            RelayError::Server(format!("响应格式无法识别: {e}"))
        })
    }

    /// POST a JSON body to a console path.
    pub async fn post_json<T: for<'de> Deserialize<'de>, B: serde::Serialize + ?Sized>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, RelayError> {
        let value = self.request(reqwest::Method::POST, path, Some(body)).await?;
        serde_json::from_value(value).map_err(|e| {
            RelayError::Server(format!("响应格式无法识别: {e}"))
        })
    }

    /// The one request path. See the module docs for the retry contract.
    async fn request<B: serde::Serialize + ?Sized>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<Value, RelayError> {
        let url = self.url_for(path)?;

        // First attempt: use the cached access token, refreshing if it is stale.
        let token = self.access_token_or_refresh().await?;
        let first = self.send(&method, &url, body, &token).await?;

        let response = match first {
            SendOutcome::Ok(value) => return Ok(value),
            SendOutcome::Unauthorized => {
                // One refresh, one replay. See the module docs.
                log::info!("[relay] access token rejected; refreshing once and retrying {path}");
                let token = self.force_refresh(true).await?;
                match self.send(&method, &url, body, &token).await? {
                    SendOutcome::Ok(value) => return Ok(value),
                    SendOutcome::Unauthorized => {
                        // The refresh succeeded but the server still refuses: the
                        // session is not recoverable from this machine.
                        log::warn!("[relay] retry after refresh was rejected; clearing session");
                        self.session.clear().await;
                        return Err(RelayError::NeedsReauth);
                    }
                    other => other,
                }
            }
            other => other,
        };

        response.into_result()
    }

    /// Send once with an explicit bearer token, classifying the outcome.
    async fn send<B: serde::Serialize + ?Sized>(
        &self,
        method: &reqwest::Method,
        url: &str,
        body: Option<&B>,
        token: &str,
    ) -> Result<SendOutcome, RelayError> {
        // Defence in depth: the URL was built from our own base, but re-check it
        // so a future change to `base` cannot silently send the session elsewhere.
        if is_relay_endpoint(url).is_none() {
            return Err(RelayError::Server(format!(
                "拒绝向非白名单主机发送会话: {url}"
            )));
        }

        let mut request = self
            .http
            .request(method.clone(), url)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .timeout(Duration::from_secs(20));

        if let Some(body) = body {
            request = request.json(body);
        }

        let response = request.send().await.map_err(|e| {
            // A transport failure is *not* an auth failure. Saying so here is what
            // keeps a laptop waking from sleep from being told to sign in.
            RelayError::Offline(e.to_string())
        })?;

        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Ok(SendOutcome::Unauthorized);
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            // A 403 is a permission problem, not a session problem: refreshing
            // would not help and would rotate a perfectly good credential.
            let message = read_error_message(response).await;
            return Err(RelayError::Rejected {
                status: 403,
                message,
            });
        }
        if status.is_server_error() {
            let message = read_error_message(response).await;
            return Err(RelayError::Server(format!("HTTP {status}: {message}")));
        }
        if !status.is_success() {
            let message = read_error_message(response).await;
            return Err(RelayError::Rejected {
                status: status.as_u16(),
                message,
            });
        }

        let body = response
            .text()
            .await
            .map_err(|e| RelayError::Offline(e.to_string()))?;
        // An empty body is a valid `200` for endpoints that only signal success.
        if body.trim().is_empty() {
            return Ok(SendOutcome::Ok(Value::Null));
        }
        let value: Value = serde_json::from_str(&body).map_err(|e| {
            // A 200 that is not JSON usually means an HTML error page from a proxy.
            RelayError::Server(format!("响应不是 JSON: {e}"))
        })?;

        // Unwrap `{code, message, data}`.
        //
        // The console puts a business code *inside* a 200: `{"code":400,"message":
        // "..."}` is a validation failure, not a success. Treating any 200 as data
        // would hand the caller a struct full of nulls and no explanation.
        if let Some(code) = value.get("code").and_then(Value::as_i64) {
            if code != 0 {
                let message = value
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("请求失败")
                    .to_string();
                return Err(RelayError::Rejected {
                    status: 200,
                    message,
                });
            }
            return Ok(SendOutcome::Ok(
                value.get("data").cloned().unwrap_or(Value::Null),
            ));
        }

        Ok(SendOutcome::Ok(value))
    }

    /// Build `{base}{path}`, rejecting anything absolute or traversal-shaped.
    fn url_for(&self, path: &str) -> Result<String, RelayError> {
        // Paths are relative to the account API (`/auth/login`, `/keys`, …). The
        // prefix is added here so a call site cannot accidentally aim the session
        // at a model endpoint by passing an absolute path.
        if path.contains("..") || path.contains("://") {
            return Err(RelayError::Server(format!(
                "内部错误：非法路径 {path}"
            )));
        }
        if !path.starts_with('/') {
            return Err(RelayError::Server(format!(
                "内部错误：路径必须以 / 开头 {path}"
            )));
        }
        // Only the account API may receive the session.
        //
        // The prefix is added here, so without this check `/v1/chat/completions`
        // would become `/api/v1/v1/chat/completions` — a 404 rather than the model
        // call it looks like, but more importantly it would mean "any path is
        // acceptable", and the next path added could carry a session token to an
        // endpoint that echoes it. An allowlist keeps that decision explicit.
        const ALLOWED_ROOTS: [&str; 5] = ["/auth/", "/keys", "/user/", "/me", "/groups"];
        if !ALLOWED_ROOTS.iter().any(|root| path.starts_with(root)) {
            return Err(RelayError::Server(format!(
                "内部错误：拒绝请求非账户接口路径 {path}"
            )));
        }
        let base_host = is_relay_endpoint(&self.base).ok_or_else(|| {
            RelayError::Server(format!("控制台地址不在白名单: {}", self.base))
        })?;
        // Only our own console may serve account endpoints.
        if base_host == RelayHost::Legacy {
            return Err(RelayError::Server(
                "旧网关不提供控制台接口".to_string(),
            ));
        }
        Ok(format!("{}{}{}", self.base, API_PREFIX, path))
    }

    /// A fresh access token, refreshing if the cached one is stale.
    async fn access_token_or_refresh(&self) -> Result<String, RelayError> {
        let now = super::now_unix();
        if let Some(token) = self.session.access_token(now).await {
            return Ok(token);
        }
        self.force_refresh(false).await
    }

    /// Refresh under the single-flight gate.
    ///
    /// Double-checks after acquiring the gate: a task that queued while another
    /// refresh was in flight must use the result rather than rotate again. This is
    /// the difference between "one refresh for five parallel requests" and "five
    /// refreshes, four of which present a rotated-away credential".
    ///
    /// # Why `replay_after_401` matters
    ///
    /// The cached token can be *locally* fresh and *server-side* dead: the session
    /// was revoked from the web console, the server restarted, or the credential
    /// rotated on another machine. In that case the 401 has already told us the
    /// cached token is worthless, and reusing it would send the same dead value
    /// twice — the retry then 401s again and the user is told to sign in even though
    /// a perfectly good refresh credential is sitting right there.
    async fn force_refresh(&self, replay_after_401: bool) -> Result<String, RelayError> {
        let _gate = self.session.begin_refresh().await;

        // Only trust the cache when we have not just been told it is invalid. After
        // a 401 the cache is known-bad, so the whole point of being here is to
        // replace it; a cache hit would defeat that.
        if !replay_after_401 {
            let now = super::now_unix();
            if let Some(token) = self.session.access_token(now).await {
                // Somebody refreshed while we waited.
                return Ok(token);
            }
        }

        let Some(refresh_token) = self.session.refresh_token().await else {
            return Err(RelayError::NeedsReauth);
        };

        let url = format!("{}{}/auth/refresh", self.base, API_PREFIX);
        if is_relay_endpoint(&url).is_none() {
            return Err(RelayError::Server(format!(
                "拒绝向非白名单主机发送会话: {url}"
            )));
        }

        let response = self
            .http
            .post(&url)
            .json(&serde_json::json!({ "refresh_token": refresh_token }))
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|e| RelayError::Offline(e.to_string()))?;

        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            log::warn!("[relay] refresh credential rejected; session ends");
            self.session.clear().await;
            return Err(RelayError::NeedsReauth);
        }
        if !status.is_success() {
            // The session may still be good; the server is just unavailable.
            return Err(RelayError::Server(format!(
                "刷新会话失败（HTTP {status}）"
            )));
        }

        let envelope: Envelope<TokenPair> = response
            .json()
            .await
            .map_err(|e| RelayError::Server(format!("刷新响应无法解析: {e}")))?;
        let refreshed = envelope.data.ok_or_else(|| {
            RelayError::Server(format!(
                "刷新被拒绝: {}",
                if envelope.message.is_empty() {
                    "未知原因".to_string()
                } else {
                    envelope.message
                }
            ))
        })?;

        let now = super::now_unix();
        self.session
            .set_access_token(
                refreshed.access_token.clone(),
                refreshed.expires_in.map(|seconds| now + seconds),
            )
            .await;
        // The server just honoured this credential, so the session is now proven —
        // not merely present on disk.
        self.session.mark_verified();

        // Persist the rotated refresh credential. A failure here is logged by
        // `set_session`; the in-memory session still works for this run.
        //
        // Read the account directly rather than through `status()`: `status()`
        // reports `signed_in: false` for an unverified session, and reading a
        // display field from a status struct meant for UI decisions is how the two
        // concerns get tangled.
        let account = self.session.account().await.unwrap_or_default();
        // A refresh response usually rotates the refresh token, but the contract
        // allows it to omit one, meaning "keep the current". Only rewrite the
        // stored session when a new token actually arrived, so the existing
        // credential is not replaced with an empty string.
        if let Some(token) = refreshed
            .refresh_token
            .filter(|token| !token.trim().is_empty())
        {
            let _ = self
                .session
                .set_session(StoredSession {
                    refresh_token: token,
                    account,
                    expires_at: refreshed
                        .refresh_expires_in
                        .map(|seconds| super::now_unix() + seconds),
                })
                .await;
        }

        Ok(refreshed.access_token)
    }

    /// Log in and persist the session.
    ///
    /// This is the console's own login — the same endpoint the website posts to.
    /// That is deliberate: one account, one session, so a user who signs in here and
    /// then opens the site finds themselves signed in, and vice versa.
    ///
    /// `device_id` / `device_label` are accepted but not sent: the current contract
    /// does not accept them, and inventing fields the server ignores is how a
    /// request quietly stops meaning what it says.
    pub async fn login(
        &self,
        account: &str,
        password: &str,
        _device_id: &str,
        _device_label: &str,
    ) -> Result<(), RelayError> {
        let url = format!("{}{}/auth/login", self.base, API_PREFIX);
        if is_relay_endpoint(&url).is_none() {
            return Err(RelayError::Server(format!(
                "拒绝向非白名单主机发送凭据: {url}"
            )));
        }

        let response = self
            .http
            .post(&url)
            .json(&serde_json::json!({
                "email": account.trim(),
                "password": password,
            }))
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| RelayError::Offline(e.to_string()))?;

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(RelayError::Rejected {
                status: 429,
                message: "尝试次数过多，请稍后再试".to_string(),
            });
        }
        if !status.is_success() {
            // The console reports bad credentials as a 4xx with a message; a 401 here
            // is a real answer, not a transport problem.
            let message = read_error_message(response).await;
            return Err(RelayError::Rejected {
                status: status.as_u16(),
                message: if message.is_empty() {
                    "账号或密码不正确".to_string()
                } else {
                    message
                },
            });
        }

        let envelope: Envelope<TokenPair> = response
            .json()
            .await
            .map_err(|e| RelayError::Server(format!("登录响应无法解析: {e}")))?;

        let tokens = envelope.data.ok_or_else(|| {
            RelayError::Rejected {
                status: 200,
                message: if envelope.message.is_empty() {
                    "登录失败".to_string()
                } else {
                    envelope.message
                },
            }
        })?;

        let now = super::now_unix();
        self.session
            .set_access_token(
                tokens.access_token,
                tokens.expires_in.map(|seconds| now + seconds),
            )
            .await;
        // A login response is proof by construction: the server just authenticated
        // this account.
        self.session.mark_verified();
        self.session
            .set_session(StoredSession {
                refresh_token: tokens.refresh_token.unwrap_or_default(),
                account: account.trim().to_string(),
                expires_at: tokens.refresh_expires_in.map(|seconds| now + seconds),
            })
            .await
            .map_err(RelayError::Server)?;

        log::info!("[relay] signed in as {}", account.trim());
        Ok(())
    }

    /// Sign out everywhere on this machine.
    ///
    /// Best-effort server call, then always clear locally. The local clear must
    /// happen even if the network is down: "sign out" that silently fails because
    /// the laptop is offline is worse than not calling the server at all.
    pub async fn logout(&self) {
        if let Some(token) = self.session.refresh_token().await {
            let url = format!("{}{}/auth/logout", self.base, API_PREFIX);
            if is_relay_endpoint(&url).is_some() {
                let _ = self
                    .http
                    .post(&url)
                    .json(&serde_json::json!({ "refresh_token": token }))
                    .timeout(Duration::from_secs(10))
                    .send()
                    .await;
            }
        }
        self.session.clear().await;
        log::info!("[relay] signed out");
    }
}

enum SendOutcome {
    Ok(Value),
    Unauthorized,
    #[allow(dead_code)]
    Other(Result<Value, RelayError>),
}

impl SendOutcome {
    fn into_result(self) -> Result<Value, RelayError> {
        match self {
            SendOutcome::Ok(value) => Ok(value),
            SendOutcome::Unauthorized => Err(RelayError::NeedsReauth),
            SendOutcome::Other(result) => result,
        }
    }
}

/// Pull a human-readable message out of an error body.
///
/// The console uses two shapes (`{"error":"…"}` and
/// `{"error":{"message":"…"}}`); a plain-text body is passed through. Anything
/// else degrades to empty so the caller uses its status-based fallback.
async fn read_error_message(response: reqwest::Response) -> String {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return format!("HTTP {status}");
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(message) = value.get("error") {
            if let Some(text) = message.as_str() {
                return text.to_string();
            }
            if let Some(text) = message.get("message").and_then(Value::as_str) {
                return text.to_string();
            }
        }
        if let Some(text) = value.get("message").and_then(Value::as_str) {
            return text.to_string();
        }
    }
    // Do not echo an HTML error page into the UI.
    if trimmed.starts_with('<') {
        return format!("HTTP {status}");
    }
    trimmed.chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::state::SessionState;
    use std::sync::Arc;

    fn client(base: &str) -> RelayClient {
        RelayClient::with_base(
            Arc::new(SessionState::with_session(None)),
            base,
        )
    }

    #[test]
    fn a_console_path_is_joined_to_the_base() {
        let client = client("https://aimyria.com");
        assert_eq!(
            client.url_for("/keys").unwrap(),
            "https://aimyria.com/api/v1/keys"
        );
    }

    /// The session must never be attached to a path outside the console API.
    #[test]
    fn non_console_paths_are_refused() {
        let client = client("https://aimyria.com");
        for path in [
            // Model traffic must never carry a console session.
            "/v1/chat/completions",
            "/chat/completions",
            "/v1/messages",
            // Absolute URLs and traversal are refused before the allowlist.
            "https://evil.example/keys",
            "/../admin",
            "",
        ] {
            assert!(
                client.url_for(path).is_err(),
                "{path} must not be accepted as a console path"
            );
        }
    }

    #[test]
    fn traversal_in_the_path_is_refused() {
        let client = client("https://aimyria.com");
        assert!(client.url_for("/../admin").is_err());
    }

    /// A base that is not one of our hosts must be refused, so a misconfigured or
    /// tampered base cannot become a credential exfiltration path.
    #[test]
    fn a_foreign_base_is_refused() {
        for base in [
            "https://evil.example",
            "https://aimyria.com.evil.example",
            "https://notaimyria.com",
        ] {
            let client = client(base);
            assert!(
                client.url_for("/keys").is_err(),
                "base {base} must be refused"
            );
        }
    }

    /// The legacy gateway does not serve console endpoints; saying so early is
    /// clearer than a 404 the user reads as "feature missing".
    #[test]
    fn the_legacy_gateway_is_not_a_console_base() {
        let client = client("https://321ai.cc");
        assert!(client.url_for("/keys").is_err());
    }

    #[test]
    fn error_codes_are_stable_and_map_to_login_visibility() {
        assert_eq!(RelayError::NeedsReauth.code(), "needs_reauth");
        assert!(RelayError::NeedsReauth.requires_login());

        let offline = RelayError::Offline("timed out".into());
        assert_eq!(offline.code(), "offline");
        assert!(
            !offline.requires_login(),
            "a network failure must never look like a logout"
        );

        let server = RelayError::Server("HTTP 502".into());
        assert_eq!(server.code(), "server_error");
        assert!(!server.requires_login());

        let rejected = RelayError::Rejected {
            status: 400,
            message: "密钥名称重复".into(),
        };
        assert_eq!(rejected.code(), "rejected");
        assert_eq!(rejected.user_message(), "密钥名称重复");
        assert!(!rejected.requires_login());
    }

    #[test]
    fn a_rejected_error_without_a_message_still_reads_sensibly() {
        let rejected = RelayError::Rejected {
            status: 418,
            message: String::new(),
        };
        assert_eq!(rejected.user_message(), "请求被拒绝（HTTP 418）");
    }
}
