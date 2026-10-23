//! End-to-end refresh behaviour against a real local HTTP server.
//!
//! # Why this is an integration test and not a unit test
//!
//! The property under test is "the caller never sees a 401, and the refresh
//! credential is rotated exactly once". Both are properties of the *interaction*
//! between the client, the session state, and a server that invalidates tokens.
//! A mocked transport would assert that our own code calls our own functions —
//! which is precisely the class of test that passes while the feature is broken.
//!
//! The server here is a small `tokio` TCP listener that speaks just enough HTTP to
//! implement the console contract. It is started inside the test, on an ephemeral
//! port, so nothing is shared between runs.
//!
//! Run with: `cargo test --lib relay::refresh_e2e`

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::relay::client::RelayClient;
use crate::relay::session::StoredSession;
use crate::relay::state::SessionState;

/// Server state the assertions read.
#[derive(Default)]
struct MockState {
    access_token: String,
    refresh_token: String,
    refresh_calls: usize,
    /// When true, `/keys` requires the *current* access token and
    /// rotates access on refresh.
    reject_next_refresh: bool,
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> (String, String, String) {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = stream.read(&mut chunk).await.unwrap_or(0);
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        // Stop once the headers are complete and, if present, the body length is met.
        if let Some(split) = find_headers_end(&buffer) {
            let head = String::from_utf8_lossy(&buffer[..split]).to_string();
            let length = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if buffer.len() >= split + 4 + length {
                return (head, String::new(), String::from_utf8_lossy(&buffer[split + 4..]).to_string());
            }
        }
    }
    (String::new(), String::new(), String::new())
}

fn find_headers_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn header_value(head: &str, name: &str) -> Option<String> {
    head.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}

/// Torn down when the test ends, not before.
///
/// The guard must outlive every request in the test, not just the server spawn:
/// the refresh happens later, and if another test flipped the switch off in the
/// meantime the client would refuse the loopback base mid-test. Returning the
/// guard alongside the base keeps the switch held for the test's whole scope.
type SwitchGuard = std::sync::MutexGuard<'static, ()>;

async fn spawn_server(
    state: Arc<tokio::sync::Mutex<MockState>>,
) -> (String, SwitchGuard, tokio::task::JoinHandle<()>) {
    // The client refuses any console base that is not one of ours. That refusal is
    // a security feature and must not be weakened for testability — so the test
    // turns on the same named switch a human would use for a local acceptance run.
    //
    // Taken under the shared lock because the variable is process-global: the
    // `hosts` module has tests that assert the *default* (off) behaviour, and
    // without serialisation the two would race.
    let guard = crate::relay::hosts::TEST_SWITCH_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::env::set_var(crate::relay::hosts::TEST_CLOCK_ENV, "1");

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let base = format!("http://{}", address);

    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let state = state.clone();
            tokio::spawn(async move {
                let (head, _, body) = read_request(&mut stream).await;
                let request_line = head.lines().next().unwrap_or("");
                let path = request_line.split_whitespace().nth(1).unwrap_or("/");
                let method = request_line.split_whitespace().next().unwrap_or("GET");
                let bearer = header_value(&head, "authorization")
                    .unwrap_or_default()
                    .trim_start_matches("Bearer ")
                    .to_string();

                let mut state = state.lock().await;

                let (status, payload) = match (method, path) {
                    ("GET", path) if path.starts_with("/api/v1/keys") => {
                        if bearer != state.access_token {
                            (401, r#"{"code":401,"message":"unauthorized"}"#.to_string())
                        } else {
                            (
                                200,
                                r#"{"code":0,"message":"success","data":{"items":[{"id":1,"name":"主力","key":"sk-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","status":"active","quota_used":0,"quota":0,"group_id":1}],"total":1}}"#.to_string(),
                            )
                        }
                    }
                    ("POST", "/api/v1/auth/refresh") => {
                        state.refresh_calls += 1;
                        let sent = serde_json::from_str::<serde_json::Value>(&body)
                            .ok()
                            .and_then(|value| {
                                value.get("refresh_token").and_then(|v| v.as_str()).map(str::to_string)
                            })
                            .unwrap_or_default();

                        if state.reject_next_refresh {
                            (401, r#"{"code":401,"message":"session_revoked"}"#.to_string())
                        } else if sent != state.refresh_token {
                            // The exact failure the single-flight gate prevents.
                            (401, r#"{"code":401,"message":"refresh_reuse_detected"}"#.to_string())
                        } else {
                            state.access_token = format!("at-{}", state.refresh_calls + 1);
                            state.refresh_token = format!("rt-{}", state.refresh_calls + 1);
                            (
                                200,
                                serde_json::json!({
                                    "code": 0,
                                    "message": "success",
                                    "data": {
                                        "access_token": state.access_token,
                                        "refresh_token": state.refresh_token,
                                        "expires_in": 900,
                                        "refresh_expires_in": 900_000,
                                        "token_type": "Bearer",
                                    }
                                })
                                .to_string(),
                            )
                        }
                    }
                    _ => (404, r#"{"error":"not_found"}"#.to_string()),
                };

                let response = format!(
                    "HTTP/1.1 {status} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    payload.len(),
                    payload
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
            });
        }
    });

    (base, guard, handle)
}

fn seed() -> Arc<tokio::sync::Mutex<MockState>> {
    Arc::new(tokio::sync::Mutex::new(MockState {
        access_token: "at-1".to_string(),
        refresh_token: "rt-1".to_string(),
        refresh_calls: 0,
        reject_next_refresh: false,
    }))
}

/// Build a client whose stored session knows a refresh credential but has no
/// access token yet — the exact state of a fresh process after a restart.
fn restarted_client(base: &str) -> RelayClient {
    let state = Arc::new(SessionState::with_session(Some(StoredSession {
        refresh_token: "rt-1".to_string(),
        account: "user@example.com".to_string(),
        expires_at: Some(crate::relay::now_unix() + 900_000),
    })));
    RelayClient::with_base(state, base)
}

/// The core requirement: after a restart, the first request transparently
/// refreshes and succeeds. The caller never sees an auth error.
#[tokio::test]
async fn a_restarted_process_refreshes_transparently_and_reads_data() {
    let mock = seed();
    let (base, _switch, server) = spawn_server(mock.clone()).await;

    let client = restarted_client(&base);
    // Drive the real key-list path, not a raw JSON read: the point is that the
    // shipped parser understands the shipped response.
    let keys = crate::relay::keys::list(&client)
        .await
        .expect("a restarted client must keep working without a login prompt");

    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].name, "主力");

    let state = mock.lock().await;
    assert_eq!(state.refresh_calls, 1, "exactly one refresh per restart");
    drop(state);
    server.abort();
}

/// A stale-but-present access token must be replaced, not sent.
#[tokio::test]
async fn an_expired_access_token_on_the_server_is_refreshed_and_replayed() {
    let mock = seed();
    let (base, _switch, server) = spawn_server(mock.clone()).await;

    // The stored session has a refresh credential, but the server's current
    // access token is something else entirely — so the first request 401s.
    let client = restarted_client(&base);
    // Prime a bogus access token so the client does not refresh up front.
    client
        .session()
        .set_access_token("stale-token".into(), Some(crate::relay::now_unix() + 900))
        .await;

    let keys = crate::relay::keys::list(&client)
        .await
        .expect("the 401 must be handled, not surfaced");

    assert_eq!(keys.len(), 1);

    let state = mock.lock().await;
    assert_eq!(
        state.refresh_calls, 1,
        "one 401 -> one refresh -> one replay"
    );
    drop(state);
    server.abort();
}

/// Several requests starting at once must share a single refresh.
///
/// This is the assertion that protects the owner's "never get logged out" rule:
/// if five callers each refreshed, four would present an already-rotated
/// credential and the server would (correctly) reject them, which the UI would
/// show as a logout.
#[tokio::test]
async fn concurrent_requests_share_one_refresh() {
    let mock = seed();
    let (base, _switch, server) = spawn_server(mock.clone()).await;

    let client = Arc::new(restarted_client(&base));

    let mut handles = Vec::new();
    for _ in 0..5 {
        let client = client.clone();
        handles.push(tokio::spawn(async move {
            crate::relay::keys::list(&client)
                .await
        }));
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(
            result.is_ok(),
            "every concurrent request must succeed: {result:?}"
        );
    }

    let state = mock.lock().await;
    assert_eq!(
        state.refresh_calls, 1,
        "five parallel requests must produce exactly one refresh, got {}",
        state.refresh_calls
    );
    drop(state);
    server.abort();
}

/// A refresh credential the server refuses ends the session, once, and reports
/// `NeedsReauth` — the only case where the UI shows a login form.
#[tokio::test]
async fn a_revoked_refresh_credential_ends_the_session_once() {
    let mock = seed();
    mock.lock().await.reject_next_refresh = true;
    let (base, _switch, server) = spawn_server(mock.clone()).await;

    let client = restarted_client(&base);
    let error = crate::relay::keys::list(&client)
        .await
        .expect_err("a revoked session must not look like success");

    match error {
        crate::relay::RelayError::NeedsReauth => {}
        other => panic!("expected NeedsReauth, got {other:?}"),
    }

    // The session must be gone locally as well, so the next launch shows the form
    // instead of retrying a dead credential forever.
    assert_eq!(
        client.session().refresh_token().await,
        None,
        "a rejected refresh must clear the stored credential"
    );

    server.abort();
}
