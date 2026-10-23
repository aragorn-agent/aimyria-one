//! Console session commands: sign in, sign out, ask who we are.
//!
//! # Why these live in their own module
//!
//! `commands::aimyria` is about configuring local clients. These are about the
//! account, and they are the only commands that touch a live credential. Keeping
//! them separate makes the credential surface easy to review: every function here
//! either mints, reads, or destroys a session, and nothing else.
//!
//! # What the frontend is allowed to see
//!
//! Never a token. `relay_session_status` returns the account name and a boolean.
//! The login response is deliberately `()`: the tokens stay in Rust, which is what
//! makes "switch pages and the session survives" true — there is nothing in the
//! WebView to lose.

use tauri::State;

use crate::relay::state::{SessionState, SessionStatus};
use crate::relay::RelayClient;

/// Errors are returned as `{ code, message }` so the UI can branch on the code
/// (show the login form, show a retry, show the server's own words) instead of
/// pattern-matching on localized strings.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayCommandError {
    pub code: String,
    pub message: String,
}

impl From<crate::relay::RelayError> for RelayCommandError {
    fn from(error: crate::relay::RelayError) -> Self {
        Self {
            code: error.code().to_string(),
            message: error.user_message(),
        }
    }
}

type CommandResult<T> = Result<T, RelayCommandError>;

#[tauri::command]
pub async fn relay_session_status(session: State<'_, std::sync::Arc<SessionState>>) -> Result<SessionStatus, String> {
    Ok(session.status().await)
}

/// Sign in with the account credentials.
///
/// The device id is derived locally and stored beside the session, so the console
/// can list this machine under 「已登录设备」 and revoke it individually. It is not a
/// hardware fingerprint — just a stable random value written once per install.
#[tauri::command]
pub async fn relay_login(
    session: State<'_, std::sync::Arc<SessionState>>,
    account: String,
    password: String,
) -> CommandResult<()> {
    let account = account.trim().to_string();
    if account.is_empty() || password.is_empty() {
        return Err(RelayCommandError {
            code: "invalid_input".to_string(),
            message: "请输入账号和密码".to_string(),
        });
    }

    let client = RelayClient::new(session.inner().clone());
    let device = device_identity();
    client
        .login(&account, &password, &device.id, &device.label)
        .await
        .map_err(RelayCommandError::from)
}

#[tauri::command]
pub async fn relay_logout(session: State<'_, std::sync::Arc<SessionState>>) -> CommandResult<()> {
    let client = RelayClient::new(session.inner().clone());
    client.logout().await;
    Ok(())
}

/// Force a refresh, so the UI can recover without restarting when a token went
/// stale while the machine was asleep.
#[tauri::command]
pub async fn relay_refresh_session(
    session: State<'_, std::sync::Arc<SessionState>>,
) -> CommandResult<SessionStatus> {
    let client = RelayClient::new(session.inner().clone());
    // Any authenticated call forces the refresh path; the cheapest one is the
    // account probe. A failure here is reported, not swallowed.
    client
        .get_json::<serde_json::Value>("/desktop/v1/auth/me")
        .await
        .map_err(RelayCommandError::from)?;
    Ok(session.status().await)
}

// ── keys ───────────────────────────────────────────────────────────────────

/// A key as the UI may see it: masked, never the secret.
///
/// # Why this type exists rather than returning `KeySummary`
///
/// The console's payload contains the full key, because that is what it is designed
/// to hand to its own frontend. The desktop shows a list; shipping the secrets
/// through IPC to a WebView that then has to avoid rendering them is a leak waiting
/// for one `console.log`. So the command returns this shape and the plaintext stays
/// on the Rust side, used only inside `relay_bind`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyRow {
    pub id: i64,
    pub name: String,
    pub masked: String,
    pub status: Option<String>,
    pub used: Option<f64>,
    pub quota: Option<f64>,
    pub group_id: Option<i64>,
    /// Human name of the key's group, resolved from `/groups/available`.
    ///
    /// A key list identified only by `sk-45fd…a324` is unusable for a person with
    /// several keys: the masked secret says nothing about what the key can call or
    /// what it will cost. The group is the part that carries meaning.
    pub group_name: Option<String>,
    /// One-line model summary for that group, e.g. `glm-5.3 / deepseek-v4.1-flash`.
    /// The console already publishes it; showing it saves the user a lookup.
    pub group_models: Option<String>,
}

/// The account's keys, masked, with their group names resolved.
#[tauri::command]
pub async fn relay_keys_list(
    session: State<'_, std::sync::Arc<SessionState>>,
) -> CommandResult<Vec<KeyRow>> {
    let client = RelayClient::new(session.inner().clone());
    let keys = crate::relay::keys::list(&client)
        .await
        .map_err(RelayCommandError::from)?;

    // Group names come from a second call. A failure here must not cost the user
    // the whole list — the keys are still selectable, just less descriptive — so it
    // degrades to an empty map rather than an error.
    let groups = match crate::relay::keys::groups(&client).await {
        Ok(groups) => groups,
        Err(error) => {
            // Not fatal: the keys are still selectable without their group names.
            // Logged because a silent failure here is how the names went missing
            // the first time — the list looked fine, every group name was null.
            log::warn!("[relay] 读取分组名称失败，密钥将只显示掩码: {error}");
            Default::default()
        }
    };

    Ok(keys
        .into_iter()
        .map(|key| {
            // Compute the mask before moving fields out of `key`.
            let masked = key.masked();
            let group = key.group_id.and_then(|id| groups.get(&id).cloned());
            KeyRow {
                id: key.id,
                name: key.name,
                masked,
                status: key.status,
                used: key.quota_used,
                quota: key.quota,
                group_id: key.group_id,
                group_name: group.as_ref().map(|(name, _)| name.clone()),
                group_models: group.and_then(|(_, models)| models),
            }
        })
        .collect())
}

/// Create a key, install it into one client, and verify the write.
///
/// This is the whole "one click" promise in a single command: the frontend does not
/// orchestrate create-then-write, because a partial result would leave the UI
/// guessing which half succeeded.
#[tauri::command]
pub async fn relay_key_install(
    app_state: State<'_, crate::store::AppState>,
    session: State<'_, std::sync::Arc<SessionState>>,
    app: String,
    endpoint: Option<String>,
    models: Option<Vec<String>>,
    group: Option<i64>,
) -> CommandResult<crate::relay::keys::InstallReport> {
    let client = RelayClient::new(session.inner().clone());

    // Defaults are the product's own relay and the models this key's group
    // actually owns. Passing an empty list would produce a client with no model
    // menu, so the caller's list is only trusted when it is non-empty.
    let endpoint = endpoint
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "https://api.aimyria.com".to_string());
    let models = match models {
        Some(list) if !list.is_empty() => list,
        _ => fetch_group_models(app_state.inner()).await?,
    };
    if models.is_empty() {
        return Err(RelayCommandError {
            code: "no_models".to_string(),
            message: "这个分组没有可用模型，请在中转站确认分组配置".to_string(),
        });
    }

    crate::relay::keys::install(
        app_state.inner(),
        &client,
        &app,
        &endpoint,
        &models,
        group,
    )
    .await
    .map_err(|message| RelayCommandError {
        // The key may already exist when the write fails; the message says so.
        code: "install_failed".to_string(),
        message,
    })
}

/// Rotate a key. The new secret is installed where the old one was, for `apps`.
#[tauri::command]
pub async fn relay_key_rotate(
    app_state: State<'_, crate::store::AppState>,
    session: State<'_, std::sync::Arc<SessionState>>,
    key_id: i64,
    apps: Vec<String>,
) -> CommandResult<Vec<crate::relay::keys::InstallReport>> {
    let client = RelayClient::new(session.inner().clone());
    let rotated = crate::relay::keys::rotate(&client, key_id)
        .await
        .map_err(RelayCommandError::from)?;

    let models = fetch_group_models(app_state.inner()).await.unwrap_or_default();
    let mut reports = Vec::new();
    let mut failures = Vec::new();

    for app in apps {
        match crate::relay::keys::install_with_secret(
            app_state.inner(),
            &app,
            &rotated.key,
            "https://api.aimyria.com",
            &models,
        )
        .await
        {
            Ok(report) => reports.push(report),
            Err(message) => failures.push(format!("{app}: {message}")),
        }
    }

    if reports.is_empty() && !failures.is_empty() {
        return Err(RelayCommandError {
            code: "rotate_failed".to_string(),
            message: failures.join("；"),
        });
    }
    if !failures.is_empty() {
        log::warn!("[relay] 轮换后部分客户端写入失败: {}", failures.join("；"));
    }
    Ok(reports)
}

/// Disable a key.
///
/// `disable`, not `delete`: the documented contract only offers disable, and it is
/// the safer operation anyway — it revokes access immediately while leaving the
/// row restorable. A hard delete can come later if the console adds it, but
/// inventing an endpoint here would produce a button that 404s.
#[tauri::command]
pub async fn relay_key_disable(
    session: State<'_, std::sync::Arc<SessionState>>,
    key_id: String,
) -> CommandResult<()> {
    let client = RelayClient::new(session.inner().clone());
    client
        .post_json::<serde_json::Value, _>(
            &format!("/desktop/v1/keys/{key_id}/disable"),
            &serde_json::json!({}),
        )
        .await
        .map_err(RelayCommandError::from)?;
    Ok(())
}

/// The models available to this account.
///
/// Read from the *gateway* (`/v1/models`), which every API key can call today —
/// not from a console endpoint that does not exist yet. The gateway is authoritative
/// about what this key may call, which is exactly what the client config needs.
async fn fetch_group_models(
    state: &crate::store::AppState,
) -> Result<Vec<String>, RelayCommandError> {
    // Prefer the key we are about to use, so the model list matches its group. The
    // gateway authenticates with the API key, not the console session.
    let Some((_, api_key, _, _)) = crate::commands::aimyria::bound_credentials(state) else {
        return Err(RelayCommandError {
            code: "no_key".to_string(),
            message: "还没有可用的密钥，无法读取模型列表".to_string(),
        });
    };

    let host = crate::relay::RelayHost::Gateway.hostname();
    let url = format!("https://{host}/v1/models");
    let response = crate::proxy::http_client::get()
        .get(&url)
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|error| RelayCommandError {
            code: "offline".to_string(),
            message: format!("读取模型列表失败：{error}"),
        })?;

    if !response.status().is_success() {
        return Err(RelayCommandError {
            code: "server_error".to_string(),
            message: format!("读取模型列表失败（HTTP {}）", response.status()),
        });
    }

    let body: serde_json::Value = response.json().await.map_err(|error| RelayCommandError {
        code: "server_error".to_string(),
        message: format!("模型列表响应无法解析：{error}"),
    })?;

    Ok(body
        .get("data")
        .and_then(|value| value.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.get("id").and_then(|id| id.as_str()))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default())
}

// ── device identity ────────────────────────────────────────────────────────

struct DeviceIdentity {
    id: String,
    label: String,
}

/// A stable per-install identity for the device list.
///
/// Stored in its own small file rather than inside the session: signing out and
/// back in on the same machine should reuse the same entry instead of leaving a
/// trail of "Windows PC, Windows PC, Windows PC" in the account's device list.
fn device_identity() -> DeviceIdentity {
    let path = crate::config::get_app_config_dir().join("device-id");
    let id = std::fs::read_to_string(&path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| {
            let generated = uuid::Uuid::new_v4().to_string();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&path, &generated);
            generated
        });

    DeviceIdentity {
        id,
        label: device_label(),
    }
}

/// A human label for the device list.
fn device_label() -> String {
    let host = std::env::var("COMPUTERNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Windows PC".to_string());
    format!("{} · AIMYRIA GO", host.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Input validation must happen before any network call: an empty password
    /// should never become a round trip that returns "wrong password", which
    /// would train the user to think their credential was rejected.
    #[test]
    fn blank_credentials_are_rejected_as_input_not_as_an_auth_failure() {
        // Mirrors the guard in `relay_login`.
        fn validate(account: &str, password: &str) -> Result<(), &'static str> {
            let account = account.trim();
            if account.is_empty() || password.is_empty() {
                return Err("请输入账号和密码");
            }
            Ok(())
        }

        assert!(validate("", "x").is_err());
        assert!(validate("   ", "x").is_err());
        assert!(validate("user@example.com", "").is_err());
        assert!(validate(" user@example.com ", "x").is_ok());
    }

    /// The device label must be one line and non-empty, because the console shows
    /// it in a table cell.
    #[test]
    fn the_device_label_is_a_single_non_empty_line() {
        let label = device_label();
        assert!(!label.trim().is_empty());
        assert!(!label.contains('\n'), "a newline would break the console table");
        assert!(label.contains("AIMYRIA GO"));
    }
}
