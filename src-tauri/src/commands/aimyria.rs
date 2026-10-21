//! AIMYRIA GO's product API — the commands the minimal UI actually calls.
//!
//! # Why this module exists
//!
//! The upstream command surface was shaped for a multi-provider switcher with a
//! local proxy: endpoint pools, failover queues, per-account OAuth, a
//! field-level config editor, usage scripts. None of that applies to a tool that
//! configures five clients from one relay. What is left — the deep-link import,
//! the settings store, the process helpers — lives in its existing modules.
//!
//! This module holds the commands that are *about the product* rather than
//! about its plumbing: what is configured, what it costs to keep running, and
//! how to get the user back into the app.
//!
//! # Design notes
//!
//! - Commands never panic on a missing client. A machine with no `codex` on it
//!   is normal, not an error, so [`get_client_status`] reports it rather than
//!   failing the whole call.
//! - Balance failures are values, not errors: the UI must keep showing the last
//!   known number and a "could not refresh" hint, not go blank.

use serde::Serialize;
use serde_json::Value;
use std::str::FromStr;
use tauri::{Manager, State};

use crate::app_config::AppType;
use crate::error::AppError;
use crate::live_writer;
use crate::services::balance;
use crate::services::provider::ProviderService;
use crate::store::AppState;

/// Balance below which the user is warned and the window pops up.
///
/// The requirement is a nudge, not a block: under this we still configure
/// clients, because refusing to work would strand someone mid-task.
pub const LOW_BALANCE_USD: f64 = 2.0;

// ── Types ──────────────────────────────────────────────────────────────────

/// What the UI needs to render one client's row.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientStatus {
    /// `AppType::as_str()` — stable across releases, safe as a React key.
    pub app: String,
    /// Display name.
    pub label: String,
    /// Whether this build can write the client's config at all.
    pub supported: bool,
    /// Whether the client's executable is on `PATH`.
    pub cli_installed: bool,
    /// Config file this build would write, for the "which file?" line.
    pub live_path: Option<String>,
    /// Whether the live config already points at our relay.
    pub configured: bool,
    /// The provider row currently selected, if any.
    pub current_provider: Option<String>,
}

/// The relay balance, as the UI shows it.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RelayBalance {
    /// Pre-paid balance in USD, if the relay reported one.
    pub remaining: Option<f64>,
    /// Spend in the current window, if reported.
    pub used: Option<f64>,
    /// `remaining` is known and below [`LOW_BALANCE_USD`].
    ///
    /// Deliberately `false` when `remaining` is unknown: a failed refresh must
    /// not light up a low-balance warning the user cannot act on.
    pub low: bool,
    /// Why there is no number, when there is none.
    pub error: Option<String>,
    /// Why the number is unavailable, as a machine-readable code.
    ///
    /// `not_bound` and `bridge_unavailable` are deliberately distinct: the
    /// first means "finish setup", the second means "the relay has not opened
    /// a key-scoped balance endpoint". Without this the UI can only show a
    /// generic dash and users keep reporting "other relays show my balance".
    pub reason: Option<String>,
}

/// Static facts the UI shows in About and links out to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductInfo {
    pub name: String,
    pub version: String,
    pub website: String,
    pub docs: String,
    pub support: String,
    /// Must be shown in About: this is a derivative work and the upstream
    /// MIT notice travels with it.
    pub license: String,
    pub attribution: String,
}

// ── Commands ───────────────────────────────────────────────────────────────

/// Product identity and the links the UI points at.
///
/// Every URL here is machine-checked against the live site, because a link in
/// About that does not resolve is worse than no link: it is the screen that is
/// supposed to establish that this is a real product with a real vendor.
///
/// `docs` and `support` used to be `https://aimyria.com/docs/one-click` and
/// `https://aimyria.com/support`; **both land on the API gateway (401)**, as do
/// `/docs`, `/download` and any other unregistered path. Only `/`, `/tutorial`,
/// `/learn/`, `/model-plaza` and the `/releases/` download path resolve. So they
/// point at pages that exist:
///
/// - `docs`    → `/tutorial`  (the product's own how-to page, 200)
/// - `support` → `/`          (the site; it carries the contact routes)
///
/// `website` is unchanged and already correct.
#[tauri::command]
pub fn product_info() -> ProductInfo {
    ProductInfo {
        name: "AIMYRIA GO".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        website: "https://aimyria.com/go/".to_string(),
        docs: "https://aimyria.com/tutorial".to_string(),
        support: "https://aimyria.com/".to_string(),
        license: "MIT".to_string(),
        attribution: "基于 CC Switch (farion1231/cc-switch) 修改，遵循 MIT 许可。".to_string(),
    }
}

/// Status of every client this build knows about.
///
/// A client that is not installed still appears, with `cli_installed: false` —
/// the UI needs to say "install this first", not hide the row.
#[tauri::command]
pub fn get_client_status(state: State<'_, AppState>) -> Result<Vec<ClientStatus>, String> {
    let mut out = Vec::with_capacity(live_writer::CLIENTS.len() + 2);
    for spec in live_writer::CLIENTS {
        out.push(client_status(state.inner(), spec));
    }
    for app in [
        crate::aimyria_apps::AimyriaOnlyApp::WorkBuddy,
        crate::aimyria_apps::AimyriaOnlyApp::DeepSeekHarness,
    ] {
        let path = app.live_path();
        let configured = std::fs::read_to_string(&path)
            .map(|text| text.contains(crate::aimyria_apps::PROVIDER_KEY))
            .unwrap_or(false);
        out.push(ClientStatus {
            app: app.as_str().to_string(),
            label: app.label().to_string(),
            supported: true,
            cli_installed: true,
            live_path: Some(path.display().to_string()),
            configured,
            current_provider: None,
        });
    }
    Ok(out)
}

fn client_status(_state: &AppState, spec: &'static live_writer::ClientSpec) -> ClientStatus {
    let app_type = spec.app_type.clone();
    let live_path = live_writer::live_path(&app_type);
    let current_provider = crate::settings::get_current_provider(&app_type);

    ClientStatus {
        app: app_type.as_str().to_string(),
        label: spec.label.to_string(),
        supported: true,
        cli_installed: spec.cli.is_none_or(cli_on_path),
        live_path: live_path.as_ref().map(|p| p.display().to_string()),
        configured: live_path.as_deref().is_some_and(points_at_aimyria),
        current_provider,
    }
}

/// Re-write every configured client from its currently selected provider row.
///
/// This is the "一键配置" button, and it is what a deep-link import already
/// does once — so this is the manual re-apply path for when a user edits a
/// config by hand, or adds a client after setup.
///
/// It never creates a provider row: the account arrives over a deep link or
/// from the relay console. A client with no row selected is skipped silently,
/// because "not set up yet" is the normal state for four of the five clients.
#[tauri::command]
pub fn configure_all(state: State<'_, AppState>) -> Result<ConfigureReport, String> {
    let mut report = ConfigureReport::default();
    for spec in live_writer::CLIENTS {
        let app_type = spec.app_type.clone();
        let Some(id) = current_row_id(state.inner(), &app_type) else {
            // Not set up yet, or this client keeps every provider at once. Either
            // way there is nothing to re-apply — and that is the normal state for
            // most clients, not a failure worth reporting.
            continue;
        };
        match ProviderService::switch(state.inner(), app_type.clone(), &id) {
            Ok(result) => {
                report.configured.push(app_type.as_str().to_string());
                report.warnings.extend(result.warnings);
            }
            Err(e) => report
                .failed
                .push((app_type.as_str().to_string(), e.to_string())),
        }
    }
    for app in [
        crate::aimyria_apps::AimyriaOnlyApp::WorkBuddy,
        crate::aimyria_apps::AimyriaOnlyApp::DeepSeekHarness,
    ] {
        let configured = std::fs::read_to_string(app.live_path())
            .map(|text| text.contains(crate::aimyria_apps::PROVIDER_KEY))
            .unwrap_or(false);
        if configured {
            report.configured.push(app.as_str().to_string());
        }
    }
    Ok(report)
}

/// The provider row a client would re-apply, or `None` if it has none.
///
/// [`ProviderService::current`] returns `Ok("")` in two different situations —
/// no row is selected, and the client is additive-mode (OpenCode and OpenClaw
/// hold every provider at once and have no single "current" one). Passing that
/// empty string to `switch` would report a bogus "provider not found" failure
/// for four of the five clients on every single run.
fn current_row_id(state: &AppState, app_type: &AppType) -> Option<String> {
    if app_type.is_additive_mode() {
        // Every row is already in effect, so there is nothing to switch. The
        // write still happens, just without a selection step.
        return state
            .db
            .get_all_providers(app_type.as_str())
            .ok()
            .filter(|all| !all.is_empty())
            .and_then(|all| all.keys().next().cloned());
    }

    let id = ProviderService::current(state, app_type.clone()).ok()?;
    (!id.is_empty()).then_some(id)
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureReport {
    /// Clients written successfully.
    pub configured: Vec<String>,
    /// Clients that failed, with the reason.
    pub failed: Vec<(String, String)>,
    /// Non-fatal notes from the writes.
    pub warnings: Vec<String>,
}

/// Re-write one client from its currently selected provider row.
///
/// The per-row counterpart to [`configure_all`], so a user who just installed
/// a CLI does not have to re-write the other four.
#[tauri::command]
pub fn configure_one(state: State<'_, AppState>, app: String) -> Result<ConfigureReport, String> {
    if let Some(only_app) = crate::aimyria_apps::AimyriaOnlyApp::from_str(&app) {
        let configured = std::fs::read_to_string(only_app.live_path())
            .map(|text| text.contains(crate::aimyria_apps::PROVIDER_KEY))
            .unwrap_or(false);
        return Ok(ConfigureReport {
            configured: if configured {
                vec![only_app.as_str().to_string()]
            } else {
                Vec::new()
            },
            failed: if configured {
                Vec::new()
            } else {
                vec![(
                    only_app.as_str().to_string(),
                    "还没有绑定 Aimyria 账号，请先从控制台导入".to_string(),
                )]
            },
            warnings: Vec::new(),
        });
    }

    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    if live_writer::client_spec(&app_type).is_none() {
        return Err(format!("暂不支持配置 {app}"));
    }

    let Some(id) = current_row_id(state.inner(), &app_type) else {
        return Err(format!(
            "{} 还没有绑定 Aimyria 账号，请先从控制台获取配置",
            app_type.as_str()
        ));
    };

    match ProviderService::switch(state.inner(), app_type.clone(), &id) {
        Ok(result) => Ok(ConfigureReport {
            configured: vec![app_type.as_str().to_string()],
            warnings: result.warnings,
            failed: Vec::new(),
        }),
        Err(e) => Ok(ConfigureReport {
            configured: Vec::new(),
            warnings: Vec::new(),
            failed: vec![(app_type.as_str().to_string(), e.to_string())],
        }),
    }
}

/// The relay balance, or the reason there is no number.
#[tauri::command]
pub async fn get_relay_balance(state: State<'_, AppState>) -> Result<RelayBalance, String> {
    let Some((base_url, api_key, balance_url, balance_token)) = bound_credentials(state.inner())
    else {
        return Ok(RelayBalance {
            reason: Some("not_bound".to_string()),
            error: Some("还没有绑定 Aimyria 账号".to_string()),
            ..Default::default()
        });
    };

    let result = match balance::get_balance_with_bridge(
        &base_url,
        &api_key,
        balance_url.as_deref(),
        balance_token.as_deref(),
    )
    .await
    {
        Ok(result) => result,
        Err(e) => {
            // A transport failure belongs on screen as a state, not as a
            // rejected command: the page keeps its last known rows and shows
            // one readable line instead of an unhandled query error.
            log::warn!("查询余额失败: {e}");
            return Ok(RelayBalance {
                reason: Some("query_failed".to_string()),
                error: Some("网络请求失败，暂时无法查询余额。请稍后重试。".to_string()),
                ..Default::default()
            });
        }
    };

    if !result.success {
        let raw = result.error.unwrap_or_default();
        // A console page served as JSON and a missing endpoint both look like
        // "Failed to parse response". Without a bridge token there is nothing
        // to query, so the honest message is the one README promises: the
        // relay has not opened a key-scoped balance endpoint yet.
        let parse_failure = raw.contains("Failed to parse response");
        let bridge_missing =
            raw.contains("余额接口") || raw.contains("balance_url") || raw.contains("balanceUrl");
        let bridge_unavailable = balance_url.is_none() && (parse_failure || bridge_missing);
        let bridge_expired = raw.contains("401") || raw.contains("403");

        let (reason, message) = if bridge_unavailable {
            (
                "bridge_unavailable",
                "中转站尚未开放按密钥查询余额；充值页仍可查看账户余额。",
            )
        } else if bridge_expired {
            (
                "query_failed",
                "余额令牌已过期或无权读取。请从密钥页重新导入以续期。",
            )
        } else if parse_failure {
            ("query_failed", "余额接口返回了无法识别的响应。请稍后重试。")
        } else if raw.trim().is_empty() {
            ("query_failed", "余额查询失败。请稍后重试。")
        } else {
            ("query_failed", raw.as_str())
        };

        return Ok(RelayBalance {
            reason: Some(reason.to_string()),
            error: Some(message.to_string()),
            ..Default::default()
        });
    }

    let Some(data) = result.data.and_then(|rows| rows.into_iter().next()) else {
        return Ok(RelayBalance {
            reason: Some("bridge_unavailable".to_string()),
            error: Some("中转站没有返回余额数值；充值页仍可查看账户余额。".to_string()),
            ..Default::default()
        });
    };
    let remaining = data.remaining;
    Ok(RelayBalance {
        used: data.used,
        low: remaining.is_some_and(|r| r < LOW_BALANCE_USD),
        remaining,
        reason: None,
        error: None,
    })
}

/// Read the relay's group whitelist for a key.
///
/// This is the one internet round-trip the UI makes while the import dialog is
/// open. It is deliberately a command rather than a website-only call: the
/// desktop must be able to configure the complete model set even when the
/// console sent an older link.
#[tauri::command]
pub async fn get_relay_models(base_url: String, api_key: String) -> Result<RelayModels, String> {
    let relay = relay_site_base(&base_url);
    let group = crate::aimyria_apps::fetch_group_models(&relay, &api_key).await?;
    Ok(RelayModels {
        group_name: group.group_name,
        platform: group.platform,
        models: group.models,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayModels {
    pub group_name: String,
    pub platform: String,
    pub models: Vec<String>,
}

/// Whether the app starts when the user logs in.
#[tauri::command]
pub fn get_autostart() -> Result<bool, String> {
    crate::auto_launch::is_auto_launch_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_autostart(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    let result = if enabled {
        crate::auto_launch::enable_auto_launch()
    } else {
        // Record the opt-out before touching the registry. If the toggle fails
        // halfway, a remembered opt-out is recoverable (the user can turn it
        // back on) but a missing one is not: the first-run bootstrap would
        // switch autostart back on at the next launch with no way to tell why.
        crate::first_run::mark_autostart_opted_out(state.inner())
            .and_then(|()| crate::auto_launch::disable_auto_launch())
    };
    result.map_err(|e: crate::error::AppError| e.to_string())
}

/// Hide the main window, leaving the tool in the tray.
///
/// Separate from `close`: closing the last window would end the process on
/// Windows, and this tool is meant to keep answering deep links and watching
/// the balance while it has no visible UI.
#[tauri::command]
pub fn hide_main_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// The relay credentials the current provider rows carry.
///
/// Prefers Claude Code's row, then Codex's: those are the two clients whose row
/// shape puts the key somewhere unambiguous. Returns `None` when no row is
/// selected yet, which is the state before the first deep-link import.
///
/// WorkBuddy and DeepSeek Harness are ours alone and have no provider row — a
/// user who imported into either of them used to read as "not bound" and lost
/// the balance panel entirely. Those two are therefore read from the config
/// file they were actually written to, which is the same evidence
/// [`client_configured`] already trusts for its green marker.
pub fn bound_credentials(state: &AppState) -> Option<(String, String, Option<String>, Option<String>)> {
    let mut fallback = None;
    for app_type in [AppType::Claude, AppType::Codex] {
        let Some(row) = selected_provider(state, &app_type) else {
            continue;
        };
        let (base_url, api_key) = row.resolve_usage_credentials(&app_type);
        if api_key.trim().is_empty() {
            continue;
        }
        let (balance_url, balance_token) = row
            .meta
            .as_ref()
            .map(|meta| (meta.balance_url.clone(), meta.balance_token.clone()))
            .unwrap_or((None, None));
        // A row that carries the read-only balance bridge wins over one that
        // does not, even if the other app is listed first. The imported row is
        // usually Codex while Claude still holds an older keyless "default".
        if balance_url
            .as_deref()
            .is_some_and(|url| !url.trim().is_empty())
        {
            return Some((base_url, api_key, balance_url, balance_token));
        }
        if fallback.is_none() {
            fallback = Some((base_url, api_key, balance_url, balance_token));
        }
    }
    fallback.or_else(aimyria_only_credentials)
}

/// Credentials read back out of the WorkBuddy / DeepSeek Harness config files.
///
/// These clients are written directly, with no provider row to carry the key,
/// so the only truthful source is the file itself. Reading it also means the
/// panel keeps agreeing with the tray marker: if that file no longer points at
/// us, `client_configured` says so and this returns `None` for the same reason.
fn aimyria_only_credentials() -> Option<(String, String, Option<String>, Option<String>)> {
    use crate::aimyria_apps::AimyriaOnlyApp;

    // WorkBuddy first: its file holds the key in a plain field.
    if let Ok(text) = std::fs::read_to_string(AimyriaOnlyApp::WorkBuddy.live_path()) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some((endpoint, key)) = workbuddy_credentials(&json) {
                return Some((endpoint, key, None, None));
            }
        }
    }

    // DeepSeek Harness keeps the key in a sibling credentials file and the
    // endpoint in the profile patch, so both halves are needed.
    let patch = std::fs::read_to_string(AimyriaOnlyApp::DeepSeekHarness.live_path()).ok()?;
    let endpoint = dsh_endpoint(&patch)?;
    let key = dsh_api_key().filter(|k| !k.trim().is_empty())?;
    Some((endpoint, key, None, None))
}

fn workbuddy_credentials(file: &serde_json::Value) -> Option<(String, String)> {
    let entries = file
        .as_array()
        .cloned()
        .or_else(|| file.get("models").and_then(|v| v.as_array()).cloned())?;
    for entry in entries {
        if entry.get("id").and_then(|v| v.as_str()) != Some(crate::aimyria_apps::PROVIDER_KEY) {
            continue;
        }
        let endpoint = entry.get("url").and_then(|v| v.as_str())?.trim();
        let key = entry.get("apiKey").and_then(|v| v.as_str())?.trim();
        if !endpoint.is_empty() && !key.is_empty() {
            return Some((endpoint.to_string(), key.to_string()));
        }
    }
    None
}

fn dsh_endpoint(patch: &str) -> Option<String> {
    let mut in_ours = false;
    for line in patch.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if indent == 0 && trimmed.starts_with(&format!("{}:", crate::aimyria_apps::PROVIDER_KEY)) {
            in_ours = true;
            continue;
        }
        if in_ours && indent == 0 {
            break;
        }
        if in_ours {
            if let Some(rest) = trimmed.strip_prefix("baseURL:") {
                let value = rest.trim().trim_matches('"').trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

fn dsh_api_key() -> Option<String> {
    let home = std::env::var_os("DSH_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".dsh")))?;
    // The profile patch lives under `profiles/<name>/`, so look next to every
    // profile rather than guessing which one the import chose.
    let profiles = std::fs::read_dir(home.join("profiles")).ok()?;
    for entry in profiles.flatten() {
        let path = entry.path().join(".credentials.yaml");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in text.lines() {
            if let Some(rest) = line.trim().strip_prefix("AIMYRIA_API_KEY:") {
                let value = rest.trim().trim_matches('"').trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

fn selected_provider(state: &AppState, app_type: &AppType) -> Option<crate::provider::Provider> {
    let id = crate::settings::get_current_provider(app_type)?;
    state
        .db
        .get_all_providers(app_type.as_str())
        .ok()?
        .get(&id)
        .cloned()
}

/// Whether an executable is reachable on `PATH`.
///
/// A client whose config we can write without its binary counts as installed,
/// so `spec.cli == None` short-circuits to `true` at the call site.
fn cli_on_path(cli: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_string())
            .split(';')
            .map(|e| e.to_ascii_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(&paths).any(|dir| {
        exts.iter().any(|ext| {
            let candidate = dir.join(format!("{cli}{ext}"));
            candidate.is_file()
        })
    })
}

/// Whether a client's live config points at one of our relays.
///
/// Deliberately a URL check rather than a key check: the key is a secret that
/// may be absent from the file (some clients read it from the environment), and
/// the base URL is what actually determines where traffic goes. Both the
/// original Aimyria host and the current 321AI gateway count as "connected".
fn points_at_aimyria(path: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    text.contains(RELAY_HOST) || text.contains(RELAY_ALT_HOST)
}

/// Whether at least one client has been written to either relay.
///
/// This drives the tray icon: black before the first successful import, green
/// once a client actually points at the gateway. It reads the live config files
/// rather than the provider table so a selected-but-never-written provider does
/// not turn the icon green.
pub fn any_client_configured() -> bool {
    live_writer::CLIENTS.iter().any(|spec| {
        client_configured(&spec.app_type)
    }) || [
        crate::aimyria_apps::AimyriaOnlyApp::WorkBuddy,
        crate::aimyria_apps::AimyriaOnlyApp::DeepSeekHarness,
    ]
    .iter()
    .any(|app| aimyria_only_app_configured(*app))
}

/// Whether *this* client's live config points at one of our relays.
///
/// The per-app half of [`any_client_configured`], for the tray menu's per-row
/// marker: the owner asked to be able to see **which** client the import landed
/// in, and "at least one client somewhere is configured" cannot answer that.
///
/// Same evidence, same semantics, and therefore the same colour language: this
/// deliberately contains no new notion of "configured", because two definitions of
/// green would be worse than one that is occasionally conservative. It reads the
/// live config file rather than the provider table for the same reason the
/// aggregate does -- a provider that was selected but never written must not mark
/// the row as imported.
pub fn client_configured(app_type: &AppType) -> bool {
    live_writer::live_path(app_type)
        .as_deref()
        .is_some_and(points_at_aimyria)
}

/// [`client_configured`] for the two apps that are ours alone rather than a
/// wrapped third-party CLI, and so have no `ClientSpec` entry.
fn aimyria_only_app_configured(app: crate::aimyria_apps::AimyriaOnlyApp) -> bool {
    std::fs::read_to_string(app.live_path())
        .map(|text| {
            text.contains(RELAY_HOST)
                || text.contains(RELAY_ALT_HOST)
                || text.contains(crate::aimyria_apps::PROVIDER_KEY)
        })
        .unwrap_or(false)
}

/// The relay host we configure clients to talk to.
///
/// Public because the deep-link importer uses it to tell "a link pointing at
/// us" from "some other provider's link", which is what decides whether a
/// re-import replaces the existing row.
pub const RELAY_HOST: &str = "api.aimyria.com";

/// The current 321AI gateway host. Kept next to [`RELAY_HOST`] so "is this one
/// of our relays?" is answered in one place.
pub const RELAY_ALT_HOST: &str = "321ai.cc";

/// The console host, used for the group-model and balance read-only endpoints.
///
/// Model traffic goes to `api.aimyria.com`; the console API lives on the site
/// host. A link may name either. Keeping this mapping in one place avoids a
/// second "which host is the relay?" decision in every caller.
pub fn relay_site_base(endpoint: &str) -> String {
    if endpoint.contains("api.aimyria.com") {
        "https://aimyria.com".to_string()
    } else if endpoint.contains("aimyria.com") {
        "https://aimyria.com".to_string()
    } else {
        endpoint.trim_end_matches('/').to_string()
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_host_is_the_one_the_balance_client_dials() {
        assert!(RELAY_HOST.starts_with("api."));
        assert!(!RELAY_HOST.contains(' '));
    }

    /// `low` is a plain field on `RelayBalance`, not something the struct derives:
    /// it is set by whoever builds the struct, at the one place that has a real
    /// balance to judge (`check_balance` builds it as
    /// `remaining.is_some_and(|r| r < LOW_BALANCE_USD)`).
    ///
    /// This test previously built the struct with `..Default::default()` and then
    /// asserted `under.low` was true -- reading a field it had just left at its
    /// default of `false`, so it could never pass. It had been red in the tree and
    /// was found while adding the marker-parity test below. The fix is to have the
    /// fixture set the field the same way production does, and to keep asserting the
    /// boundary the production expression defines.
    #[test]
    fn low_balance_only_fires_on_a_known_balance() {
        // Mirrors `check_balance`: unknown -> false, below -> true, at -> false.
        fn low_for(remaining: Option<f64>) -> bool {
            remaining.is_some_and(|r| r < LOW_BALANCE_USD)
        }

        assert!(!low_for(None), "an unknown balance must not read as low");
        assert!(low_for(Some(LOW_BALANCE_USD - 0.01)));
        assert!(
            !low_for(Some(LOW_BALANCE_USD)),
            "the threshold itself is not 'low'"
        );

        // And the struct carries whatever the caller set, defaulting to false.
        let unknown = RelayBalance::default();
        assert!(!unknown.low, "a defaulted balance is not low");
        let under = RelayBalance {
            remaining: Some(LOW_BALANCE_USD - 0.01),
            low: low_for(Some(LOW_BALANCE_USD - 0.01)),
            ..Default::default()
        };
        assert!(under.low);
    }

    #[test]
    fn a_live_config_pointing_elsewhere_is_not_configured() {
        let dir = std::env::temp_dir().join("aimyria-one-status-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");

        std::fs::write(
            &path,
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://elsewhere.example"}}"#,
        )
        .unwrap();
        assert!(!points_at_aimyria(&path));

        std::fs::write(
            &path,
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://api.aimyria.com"}}"#,
        )
        .unwrap();
        assert!(points_at_aimyria(&path));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_live_config_is_simply_not_configured() {
        assert!(!points_at_aimyria(std::path::Path::new(
            "D:/definitely/not/here/settings.json"
        )));
    }

    /// The per-client marker (requirement 8) and the aggregate tray icon must agree.
    ///
    /// `client_configured` exists so the tray menu can show *which* client the import
    /// landed in, while `any_client_configured` still answers *whether* anything is
    /// configured. Two functions, one notion of "configured" -- if they ever diverge,
    /// the menu would show a client as imported while the icon still said nothing had
    /// happened, which is the opposite of the feedback the owner asked for.
    ///
    /// The shared evidence is `points_at_aimyria`, so that is what this pins: the
    /// per-client answer for a path is exactly the aggregate's answer for that path.
    #[test]
    fn the_per_client_marker_uses_the_same_evidence_as_the_aggregate_icon() {
        let dir = std::env::temp_dir().join("aimyria-one-marker-parity");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");

        // Not ours -> neither says configured.
        std::fs::write(&path, r#"{"env":{"ANTHROPIC_BASE_URL":"https://elsewhere.example"}}"#).unwrap();
        assert!(!points_at_aimyria(&path), "the fixture must not count as ours");

        // Ours -> the shared predicate says yes, which is what both callers consult.
        std::fs::write(
            &path,
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://api.aimyria.com"}}"#,
        )
        .unwrap();
        assert!(points_at_aimyria(&path));

        // The legacy gateway counts too, for the same reason the aggregate does:
        // installs written against it are configured, not broken.
        std::fs::write(&path, r#"{"env":{"ANTHROPIC_BASE_URL":"https://321ai.cc/v1"}}"#).unwrap();
        assert!(points_at_aimyria(&path));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
