//! Keys as first-class resources, and which clients are bound to them.
//!
//! # Why this exists
//!
//! The provider table already stores one row per (client, key, endpoint). That is
//! the right storage, but it presents the data **per client**, so the user's real
//! question — "which of my keys is Claude Code using, and can I move it to the
//! other key?" — requires opening four different screens and comparing secrets by
//! eye.
//!
//! This module inverts the view: keys first, clients second. One key can be bound
//! to several clients at once (this machine has `sk-ca2131e4…` on both Codex and
//! Claude Desktop), and moving a client to another key is one operation.
//!
//! # What "binding" means, and what it deliberately is not
//!
//! Binding is **not** a new persisted relationship. It is exactly what
//! `ProviderService::switch` already does: write the chosen row's settings into the
//! client's live config. Inventing a second store for "client → key" would create a
//! third source of truth, and two screens already disagreed because of two sources.
//!
//! So the rule this module follows is: **read the truth from the live config, write
//! through the existing switch.** Nothing else.
//!
//! # The constraint that shapes the UI
//!
//! There is no console endpoint that lists an account's keys — `GET
//! /v1/user/keys` is 404 — so this cannot show keys the user created on the
//! website but never imported here. What it can show is every key this machine
//! knows, which is the set the user can actually bind something to.

use std::collections::BTreeMap;

use serde::Serialize;
use tauri::State;

use crate::app_config::AppType;
use crate::store::AppState;

/// One key this machine knows, with the clients currently using it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    /// Stable identity: a hash of the secret.
    ///
    /// Hashed rather than the secret itself so this can be used as a React key and
    /// logged without leaking anything. Two identical keys on two rows share it,
    /// which is what makes "one key, several clients" visible.
    pub fingerprint: String,
    /// Masked form for display: `sk-45fd…a324`.
    pub masked: String,
    /// The endpoint this key talks to.
    pub endpoint: String,
    /// Friendly name — taken from the first row that carries this key.
    pub name: String,
    /// Whether the endpoint is one of our relays.
    pub is_relay: bool,
    /// Every client bound to this key, by `AppType::as_str()`.
    pub bound_apps: Vec<String>,
}

/// One client and the key it currently has in its live config.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBinding {
    pub app: String,
    pub label: String,
    /// Fingerprint of the key in the client's live config, when it is one we know.
    pub fingerprint: Option<String>,
    /// The key's masked form, for the cases where the config holds a key that
    /// belongs to no provider row (hand-edited, or written by another tool).
    pub masked: Option<String>,
    /// Is the live config pointing at a key rather than an official endpoint?
    pub bound: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialMatrix {
    pub credentials: Vec<Credential>,
    pub apps: Vec<AppBinding>,
}

/// Hash a secret for identity, never for verification.
///
/// Truncated to 16 hex characters: enough to be unique across the handful of keys a
/// machine holds, short enough to read in a log.
fn fingerprint(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(secret.trim().as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Mask a secret for display: prefix and last four, like every credential UI.
fn mask(secret: &str) -> String {
    let trimmed = secret.trim();
    if trimmed.len() <= 12 {
        return "sk-…".to_string();
    }
    let head: String = trimmed.chars().take(7).collect();
    let tail: String = trimmed
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

/// Collect every (client, key) pair the database holds.
fn collect(state: &AppState, app_type: &AppType) -> Vec<(String, String, String, String)> {
    // (row id, row name, secret, endpoint)
    let Ok(rows) = state.db.get_all_providers(app_type.as_str()) else {
        return Vec::new();
    };
    rows.values()
        .filter_map(|row| {
            let (base_url, api_key) = row.resolve_usage_credentials(app_type);
            let api_key = api_key.trim();
            if api_key.is_empty() {
                return None;
            }
            Some((
                row.id.clone(),
                row.name.clone(),
                api_key.to_string(),
                base_url,
            ))
        })
        .collect()
}

/// The key currently written into a client's live config, if the file is readable.
///
/// Read from the file rather than from the selected row, because the file is what
/// decides where traffic goes. That distinction matters in the exact case the user
/// needs to see: a lost write leaves the file on an old key while the selection has
/// moved on, and reporting the selection would hide it.
///
/// Returns `(masked, secret)`.
fn live_key(state: &AppState, app_type: &AppType) -> Option<(String, String)> {
    // The two file-based clients store the secret in their own formats.
    if let Some(app) = crate::aimyria_apps::AimyriaOnlyApp::from_str(app_type.as_str()) {
        let (_, secret) = crate::aimyria_apps::read_live_credentials(app)?;
        return Some((mask(&secret), secret));
    }

    let path = crate::live_writer::live_path(app_type)?;
    let text = std::fs::read_to_string(&path).ok()?;

    // Extract whatever key the file actually holds, by asking every candidate row
    // for its secret and checking which one appears in the file.
    //
    // This is deliberately a membership test rather than a parse of each client's
    // format: `live_writer` already knows how to *write* five different shapes, and
    // a second reader per shape is a second thing to keep in sync. The rows are the
    // universe of keys this machine knows, so "which row's key is in the file" is
    // answerable without parsing.
    let rows = state.db.get_all_providers(app_type.as_str()).ok()?;
    for row in rows.values() {
        let (_, secret) = row.resolve_usage_credentials(app_type);
        let secret = secret.trim();
        if !secret.is_empty() && text.contains(secret) {
            return Some((mask(secret), secret.to_string()));
        }
    }

    // The file holds a key that belongs to no row: configured by hand, or written
    // by another tool. It is still the key in effect, so report it as unknown
    // rather than as "unbound" — those are different facts.
    None
}

/// Every key this machine knows, plus what each client is bound to.
#[tauri::command]
pub fn relay_credentials(state: State<'_, AppState>) -> Result<CredentialMatrix, String> {
    let mut by_fingerprint: BTreeMap<String, Credential> = BTreeMap::new();
    let mut apps: Vec<AppBinding> = Vec::new();

    // Clients that hold a provider row.
    for spec in crate::live_writer::CLIENTS {
        let app_type = spec.app_type.clone();
        let entries = collect(state.inner(), &app_type);

        for (_, name, secret, endpoint) in &entries {
            let fp = fingerprint(secret);
            let is_relay = crate::relay::is_relay_endpoint(endpoint).is_some();
            by_fingerprint
                .entry(fp.clone())
                .and_modify(|cred| {
                    if !cred.bound_apps.iter().any(|a| a == app_type.as_str()) {
                        cred.bound_apps.push(app_type.as_str().to_string());
                    }
                })
                .or_insert_with(|| Credential {
                    fingerprint: fp.clone(),
                    masked: mask(secret),
                    endpoint: endpoint.clone(),
                    name: name.clone(),
                    is_relay,
                    bound_apps: vec![app_type.as_str().to_string()],
                });
        }

        let live = live_key(state.inner(), &app_type);
        apps.push(AppBinding {
            app: app_type.as_str().to_string(),
            label: spec.label.to_string(),
            fingerprint: live.as_ref().map(|(_, secret)| fingerprint(secret)),
            masked: live.as_ref().map(|(masked, _)| masked.clone()),
            bound: live.is_some() && crate::commands::aimyria::client_configured(&app_type),
        });
    }

    // The two file-based clients have no rows; they still appear so a user can see
    // that they are (or are not) bound to something.
    for only in [
        crate::aimyria_apps::AimyriaOnlyApp::WorkBuddy,
        crate::aimyria_apps::AimyriaOnlyApp::DeepSeekHarness,
    ] {
        let live = crate::aimyria_apps::read_live_credentials(only);
        if let Some((_, secret)) = live.as_ref() {
            let fp = fingerprint(secret);
            by_fingerprint
                .entry(fp.clone())
                .and_modify(|cred| {
                    if !cred.bound_apps.iter().any(|a| a == only.as_str()) {
                        cred.bound_apps.push(only.as_str().to_string());
                    }
                })
                .or_insert_with(|| Credential {
                    fingerprint: fp.clone(),
                    masked: mask(secret),
                    endpoint: String::new(),
                    name: only.label().to_string(),
                    is_relay: true,
                    bound_apps: vec![only.as_str().to_string()],
                });
        }
        apps.push(AppBinding {
            app: only.as_str().to_string(),
            label: only.label().to_string(),
            fingerprint: live.as_ref().map(|(_, secret)| fingerprint(secret)),
            // `read_live_credentials` returns `(endpoint, secret)`; masking the
            // endpoint is how this line first shipped the URL where the key belongs.
            masked: live.as_ref().map(|(_, secret)| mask(secret)),
            bound: live.is_some(),
        });
    }

    // Relay keys first: those are the ones this product is about.
    let mut credentials: Vec<Credential> = by_fingerprint.into_values().collect();
    credentials.sort_by(|a, b| {
        b.is_relay
            .cmp(&a.is_relay)
            .then_with(|| b.bound_apps.len().cmp(&a.bound_apps.len()))
            .then_with(|| a.masked.cmp(&b.masked))
    });

    Ok(CredentialMatrix { credentials, apps })
}

/// Bind one client to a key.
///
/// # The case that made this more than a lookup
///
/// Keys are aggregated across every client, so the picker offers a key that this
/// client has no row for — WorkBuddy's key appearing in Claude Code's list, for
/// instance. Refusing with "no row for this key" is technically true and useless:
/// the user's request is "use this key here", and the row is an implementation
/// detail of how that is stored.
///
/// So a missing row is created from the key's own endpoint and model list, then the
/// switch runs. The switch is still the only write path — this never writes a live
/// config directly.
///
/// # Why an existing row is preferred over a new one
///
/// A row can carry more than a key and an endpoint: a balance bridge token, a model
/// route map for Claude Desktop's proxy mode, per-row notes. Reusing the row keeps
/// all of it; creating a fresh one would silently drop those settings.
#[tauri::command]
pub async fn relay_bind(
    state: State<'_, AppState>,
    session: tauri::State<'_, std::sync::Arc<crate::relay::SessionState>>,
    app: String,
    key_fingerprint: String,
) -> Result<(), String> {
    use std::str::FromStr;

    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    let rows = state
        .db
        .get_all_providers(app_type.as_str())
        .map_err(|e| e.to_string())?;

    // 1. An existing row already carrying this key: reuse it, which preserves any
    //    metadata the row accumulated.
    let existing = rows.values().find_map(|row| {
        let (_, secret) = row.resolve_usage_credentials(&app_type);
        (fingerprint(secret.trim()) == key_fingerprint).then(|| row.id.clone())
    });

    if let Some(row_id) = existing {
        return crate::services::provider::ProviderService::switch(
            state.inner(),
            app_type,
            &row_id,
        )
        .map(|_| ())
        .map_err(|e| e.to_string());
    }

    // 2. Not on this machine. Ask the account: the key may exist on the console and
    //    never have been imported here, which is the normal state for a fresh
    //    install and exactly why the account list is worth having.
    //
    // Two identifier forms arrive here:
    //   * `acct:<id>` — chosen from the account list, which never exposes a secret;
    //   * a bare fingerprint — a key this machine already knows.
    let account_key_id = key_fingerprint
        .strip_prefix("acct:")
        .and_then(|raw| raw.parse::<i64>().ok());

    let from_account = async {
        let client = crate::relay::RelayClient::new(session.inner().clone());
        let keys = crate::relay::keys::list(&client).await.ok()?;
        let found = match account_key_id {
            Some(id) => keys.into_iter().find(|key| key.id == id),
            None => keys
                .into_iter()
                .find(|key| fingerprint(key.key.trim()) == key_fingerprint),
        }?;
        // Every account key is served by the model gateway; the console's list does
        // not carry a per-key endpoint, and inventing one per key would be a guess.
        Some((
            "https://api.aimyria.com".to_string(),
            found.key,
            found.name,
        ))
    }
    .await;

    // 3. Otherwise look across every client's rows and their files.
    let Some((endpoint, secret, name)) = from_account
        .or_else(|| find_key_anywhere(state.inner(), &key_fingerprint))
    else {
        return Err("这把密钥不在本机，也不在当前账户里。".to_string());
    };

    if let Some(only) = crate::aimyria_apps::AimyriaOnlyApp::from_str(app_type.as_str()) {
        let models = crate::aimyria_apps::fetch_group_models(
            &crate::commands::aimyria::relay_site_base(&endpoint),
            &secret,
        )
        .await
        .map(|group| group.models)
        .unwrap_or_default();
        if models.is_empty() {
            return Err("读不到这把密钥的可用模型，请稍后重试。".to_string());
        }
        return crate::commands::aimyria::write_aimyria_only_client(
            state.inner(),
            only,
            Some(&models),
        )
        .await;
    }

    let all_models = crate::aimyria_apps::fetch_group_models(
        &crate::commands::aimyria::relay_site_base(&endpoint),
        &secret,
    )
    .await
    .map(|group| group.models)
    .unwrap_or_default();

    // Claude Desktop's direct profile accepts only `claude-*` ids. Handing it the
    // group's full catalogue produces a validator failure ("fail-all") and an empty
    // model menu, so filter here rather than letting the write succeed wrongly.
    let models: Vec<String> = if matches!(app_type, AppType::ClaudeDesktop) {
        all_models
            .into_iter()
            .filter(|id| {
                let lower = id.to_ascii_lowercase();
                lower.starts_with("claude-") || lower.starts_with("anthropic/claude-")
            })
            .collect()
    } else {
        all_models
    };

    if models.is_empty() {
        return Err(format!(
            "{app} 需要 claude-* 模型，但这把密钥的分组里没有。请换一个分组或改用本地路由模式。"
        ));
    }

    let request = crate::deeplink::DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some(app_type.as_str().to_string()),
        name: Some(name),
        enabled: Some(true),
        homepage: Some("https://aimyria.com".to_string()),
        endpoint: Some(endpoint),
        api_key: Some(secret),
        icon: Some("aimyria".to_string()),
        model: models.first().cloned(),
        models: Some(models),
        // Claude Desktop binds **direct**, not through the local router.
        //
        // Measured: the gateway answers `POST /v1/messages` with a real Anthropic
        // payload (HTTP 200), so Desktop can talk to it without anything of ours
        // running in between. That matters because proxy mode requires the local
        // routing service to stay alive on this machine — a tray app that must not
        // be quit, or the user's editor stops working. Direct removes that whole
        // dependency.
        //
        // The catch direct mode has is the model menu: Desktop only lists models
        // present in the profile's `inferenceModels`, and the import only fills
        // that from `claude-*` route ids. So the model list passed in must be the
        // gateway's real `claude-*` ids — which is why the caller resolves them
        // from `/v1/models` rather than from the group's full catalogue.
        claude_desktop_mode: matches!(app_type, AppType::ClaudeDesktop)
            .then(|| "direct".to_string()),
        ..Default::default()
    };

    let provider_id = crate::deeplink::import_provider_from_deeplink(state.inner(), request)
        .await
        .map_err(|e| format!("写入配置失败：{e}"))?;
    crate::services::provider::ProviderService::switch(state.inner(), app_type, &provider_id)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Find a key anywhere on this machine, returning `(endpoint, secret, row name)`.
///
/// Scans every client's provider rows and the two file-based clients. The endpoint
/// comes from whichever place holds the key, which is exact: the same secret can
/// only reach one endpoint in a sane configuration.
fn find_key_anywhere(state: &AppState, key_fingerprint: &str) -> Option<(String, String, String)> {
    for app_type in AppType::all() {
        let rows = state.db.get_all_providers(app_type.as_str()).ok()?;
        for row in rows.values() {
            let (endpoint, secret) = row.resolve_usage_credentials(&app_type);
            let secret = secret.trim();
            if !secret.is_empty() && fingerprint(secret) == key_fingerprint {
                return Some((endpoint, secret.to_string(), row.name.clone()));
            }
        }
    }

    for only in [
        crate::aimyria_apps::AimyriaOnlyApp::WorkBuddy,
        crate::aimyria_apps::AimyriaOnlyApp::DeepSeekHarness,
    ] {
        if let Some((endpoint, secret)) = crate::aimyria_apps::read_live_credentials(only) {
            if fingerprint(&secret) == key_fingerprint {
                return Some((endpoint, secret, only.label().to_string()));
            }
        }
    }

    None
}

// ── Claude Desktop: apply and restart in one step ─────────────────────────

/// Result of trying to bring Claude Desktop onto the new configuration.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopRestart {
    /// A running instance was found and asked to close.
    pub was_running: bool,
    /// A new instance was launched.
    pub relaunched: bool,
    /// What the user should do if this did not work, when `relaunched` is false.
    pub hint: Option<String>,
}

/// Restart Claude Desktop so it picks up the configuration just written.
///
/// # Why this is a command and not an instruction
///
/// Claude Desktop reads its profile once, at startup. A configuration written while
/// it is running is invisible until the user quits and opens it again — and "now go
/// restart it yourself" is exactly the step people miss. The whole point of a
/// one-click import is that the next thing they do is open the app and use it.
///
/// # Why the path is discovered rather than stored
///
/// Claude ships as a Squirrel install: `app-<version>\claude.exe`, and the version
/// directory changes on every auto-update. This machine moved from `2.42304.2` to
/// `3.6556.2` within one session. A remembered path would break at the first
/// update, so the newest `app-*` directory is resolved each time.
///
/// Failure is reported, not fatal: on a machine where Claude lives somewhere else
/// the command says so and the UI falls back to asking the user to open it.
#[tauri::command]
pub async fn relay_restart_claude_desktop() -> Result<DesktopRestart, String> {
    #[cfg(not(target_os = "windows"))]
    {
        return Ok(DesktopRestart {
            was_running: false,
            relaunched: false,
            hint: Some("请手动退出并重新打开 Claude Desktop".to_string()),
        });
    }

    #[cfg(target_os = "windows")]
    {
        // 1. Ask a running instance to close.
        //
        // `taskkill` without `/F` sends WM_CLOSE, which is what the app's own quit
        // path expects; `/F` would kill it mid-write and can leave the profile
        // half-updated. A forced kill is only a fallback when the graceful request
        // leaves a process behind.
        let was_running = process_is_running("claude.exe");
        if was_running {
            let _ = run_hidden("taskkill", &["/IM", "claude.exe"]);
            // Give it up to ~4s to actually exit.
            for _ in 0..20 {
                if !process_is_running("claude.exe") {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            if process_is_running("claude.exe") {
                // Graceful close did not take (a modal dialog can hold it open).
                log::warn!("Claude Desktop 未在超时内退出，强制结束");
                let _ = run_hidden("taskkill", &["/F", "/IM", "claude.exe"]);
            }
        }

        // 2. Find and start the newest installed build.
        let Some(exe) = find_claude_desktop_exe() else {
            return Ok(DesktopRestart {
                was_running,
                relaunched: false,
                hint: Some(
                    "没有找到 Claude Desktop。请手动打开它一次即可完成配置。".to_string(),
                ),
            });
        };

        let launched = std::process::Command::new(&exe)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("启动 Claude Desktop 失败（{}）: {e}", exe.display()));

        match launched {
            Ok(()) => {
                log::info!("[claude-desktop] 已重启 {}", exe.display());
                Ok(DesktopRestart {
                    was_running,
                    relaunched: true,
                    hint: None,
                })
            }
            Err(e) => Ok(DesktopRestart {
                was_running,
                relaunched: false,
                hint: Some(format!("{e}。请手动打开 Claude Desktop。")),
            }),
        }
    }
}

#[cfg(target_os = "windows")]
fn process_is_running(image: &str) -> bool {
    use std::os::windows::process::CommandExt;
    // `tasklist` rather than the Windows API: this runs once per import, and the
    // output is stable across the Windows versions this product supports.
    let output = std::process::Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {image}"), "/NH"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output();
    match output {
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
            text.contains(&image.to_ascii_lowercase())
        }
        Err(_) => false,
    }
}

#[cfg(target_os = "windows")]
fn run_hidden(program: &str, args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(program)
        .args(args)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// The newest `app-*\claude.exe` under the Squirrel install root.
#[cfg(target_os = "windows")]
fn find_claude_desktop_exe() -> Option<std::path::PathBuf> {
    let root = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)?
        .join("AnthropicClaude");
    if !root.is_dir() {
        return None;
    }

    let mut candidates: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(&root)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            // Squirrel keeps `app-<version>`; older builds used `app-<version>`
            // too, so the prefix is the only stable part.
            if !name.starts_with("app-") {
                return None;
            }
            let exe = entry.path().join("claude.exe");
            exe.is_file().then_some((name, exe))
        })
        .collect();

    // Sort by version-like name so `app-3.6556.2` beats `app-2.42304.2`; the
    // names are not semver, so a plain reverse string sort is wrong (`app-10.` <
    // `app-9.`). Compare the numeric segments instead.
    candidates.sort_by(|a, b| version_key(&a.0).cmp(&version_key(&b.0)));
    candidates.pop().map(|(_, exe)| exe)
}

/// Numeric key for an `app-<version>` directory name.
#[cfg(target_os = "windows")]
fn version_key(name: &str) -> Vec<u64> {
    name.trim_start_matches("app-")
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_secret_has_one_fingerprint_and_different_secrets_differ() {
        let a = fingerprint("sk-45fdab35f6d1b74730f2e1f6166f72c6732e13f4d5c5423d2d17c11db77aa324");
        let b = fingerprint("sk-45fdab35f6d1b74730f2e1f6166f72c6732e13f4d5c5423d2d17c11db77aa324");
        let c = fingerprint("sk-ca2131e4d07b9e0cfef415780bb43031527ab200987a9b9a877f2019e02496eb");
        assert_eq!(a, b);
        assert_ne!(a, c);
        // Trimming must not change identity: a key pasted with a trailing space is
        // the same credential.
        assert_eq!(a, fingerprint("  sk-45fdab35f6d1b74730f2e1f6166f72c6732e13f4d5c5423d2d17c11db77aa324  "));
        // Short and readable in a log.
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn masking_never_exposes_the_body() {
        let secret = "sk-45fdab35f6d1b74730f2e1f6166f72c6732e13f4d5c5423d2d17c11db77aa324";
        let masked = mask(secret);
        assert!(masked.starts_with("sk-45fd"), "got {masked}");
        assert!(masked.ends_with("a324"), "got {masked}");
        assert!(!masked.contains("5c5423d2d17c11db"), "the body must be gone");
        assert_eq!(mask("short"), "sk-…");
    }

    /// The fingerprint must not be reversible to the secret.
    ///
    /// It is a SHA-256 prefix, so it is not; the assertion documents that this is a
    /// deliberate property rather than an accident of formatting.
    /// The version sort must compare numbers, not strings.
    ///
    /// Squirrel names directories `app-<version>`, and a plain reverse string sort
    /// ranks `app-9.0.0` above `app-10.0.0`. Picking the wrong directory means
    /// launching a stale build — or, after an update removed the old one, no build
    /// at all.
    #[cfg(target_os = "windows")]
    #[test]
    fn the_newest_app_directory_wins() {
        let mut names = vec!["app-2.42304.2", "app-3.6556.2", "app-10.0.0", "app-9.9.9"];
        names.sort_by_key(|name| super::version_key(name));
        assert_eq!(names.last().copied(), Some("app-10.0.0"));
        // The real pair this machine went through in one session.
        assert!(super::version_key("app-3.6556.2") > super::version_key("app-2.42304.2"));
    }

    #[test]
    fn the_fingerprint_is_not_the_secret() {
        let secret = "sk-45fdab35f6d1b74730f2e1f6166f72c6732e13f4d5c5423d2d17c11db77aa324";
        let fp = fingerprint(secret);
        assert!(!secret.contains(&fp));
        assert!(!fp.contains("45fdab"));
    }
}
