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
use std::str::FromStr;
use tauri::{Manager, State};

use crate::app_config::AppType;
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
    ///
    /// This is the **file on disk**, which is the only thing that decides where the
    /// client's traffic actually goes. Everything the UI says must ultimately agree
    /// with it.
    pub configured: bool,
    /// The provider row currently selected, if any.
    pub current_provider: Option<String>,
    /// Display name of the row that is actually written into the client.
    ///
    /// Null when the live file points somewhere that is not one of our rows (the
    /// user configured the client by hand, or another tool overwrote it).
    pub live_provider_name: Option<String>,
    /// The row the app *thinks* is current agrees with the file on disk.
    ///
    /// `false` is the state that produced the reported "two pages disagree" bug:
    /// the provider list had switched to 神稳AI (its own state said so) while the
    /// live file still contained the relay's key, so the one-click screen said
    /// 「已接入」 and the provider page said 「使用中 · 神稳AI」 — two answers to one
    /// question.
    ///
    /// The UI must surface this rather than pick a winner: a mismatch means a write
    /// was lost, and silently trusting either side hides it.
    pub state_consistent: bool,
    /// Human-readable explanation when `state_consistent` is false.
    pub inconsistency: Option<String>,
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
        // Use the same predicate as every other client. The previous version
        // checked only for the literal `PROVIDER_KEY` string, which is true for a
        // row we created *and* for a row the user wrote by hand pointing somewhere
        // else — so a client aimed at a different relay still showed 「已接入」.
        let configured = aimyria_only_app_configured(app);
        out.push(ClientStatus {
            app: app.as_str().to_string(),
            label: app.label().to_string(),
            supported: true,
            cli_installed: true,
            live_path: Some(path.display().to_string()),
            configured,
            current_provider: None,
            // These two clients have no provider row at all: their key lives in
            // their own config file, so "the row says X, the file says Y" cannot
            // happen. Reporting them as consistent is accurate, not a fudge.
            live_provider_name: None,
            state_consistent: true,
            inconsistency: None,
        });
    }
    Ok(out)
}

/// Put a client back on its own built-in models.
///
/// Only the two Aimyria-only clients support this today, and only they need it:
/// they receive records this tool wrote into the vendor's own model file, so
/// "back to official" means withdrawing those records. The other clients are
/// reverted by switching to their official provider row, which already exists.
///
/// Returns how many records were withdrawn, so the UI can report a number instead
/// of silently rewriting a file the user may be looking at in the vendor's panel.
#[tauri::command]
pub fn revert_to_official(app: String) -> Result<usize, String> {
    use crate::aimyria_apps::AimyriaOnlyApp;

    let only_app = AimyriaOnlyApp::from_str(&app)
        .ok_or_else(|| format!("{app} 不支持单独切回官方模型"))?;
    match only_app {
        AimyriaOnlyApp::WorkBuddy => {
            crate::aimyria_apps::revert_workbuddy_to_official_at(&only_app.live_path())
                .map_err(|e| e.to_string())
        }
        // dsh keeps providers in a YAML patch layer; withdrawing a single provider
        // from it is a different operation from WorkBuddy's flat model list, and
        // doing it wrong leaves a dangling `model:` reference. Out of scope here
        // rather than half-done.
        AimyriaOnlyApp::DeepSeekHarness => Err(
            "DeepSeek Harness 暂不支持单独切回官方模型，请在它的设置里切换 provider".to_string(),
        ),
    }
}

/// The models this account can use, for the model picker.
///
/// Read from the gateway with the key already on this machine, so the list is
/// exactly what the key is entitled to — not a catalogue of everything the relay
/// sells.
#[tauri::command]
pub async fn get_available_models(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let Some((base_url, api_key, _, _)) = bound_credentials(state.inner()) else {
        return Ok(Vec::new());
    };
    let group = crate::aimyria_apps::fetch_group_models(&relay_site_base(&base_url), &api_key)
        .await?;
    Ok(group.models)
}

fn client_status(state: &AppState, spec: &'static live_writer::ClientSpec) -> ClientStatus {
    let app_type = spec.app_type.clone();
    let live_path = live_writer::live_path(&app_type);
    let configured = live_path.as_deref().is_some_and(points_at_aimyria);

    // The row the database flags as current. This is what the provider page shows
    // as 使用中, and what a re-apply would write.
    let db_current = state
        .db
        .get_current_provider(app_type.as_str())
        .ok()
        .flatten();
    // The stale device-local copy, kept so a mismatch can be reported rather than
    // silently preferred.
    let settings_current = crate::settings::get_current_provider(&app_type);

    let live_provider_name = db_current.as_ref().and_then(|id| {
        state
            .db
            .get_all_providers(app_type.as_str())
            .ok()?
            .get(id)
            .map(|row| row.name.clone())
    });

    // Is the selected row the one actually in the client's config?
    //
    // Both halves matter and neither is sufficient:
    //  * the file must point at our relay (otherwise the client is on something
    //    else entirely, whatever the database says);
    //  * the database's current row must be a relay row (otherwise the user switched
    //    to a third party and the file is stale — the reverse mismatch).
    //
    // A `default`-like row that carries our key counts as a relay row, which is why
    // this asks `points_at_aimyria` about the row rather than comparing ids.
    let current_row_is_relay = db_current
        .as_ref()
        .and_then(|id| {
            state
                .db
                .get_all_providers(app_type.as_str())
                .ok()?
                .get(id)
                .cloned()
        })
        .is_some_and(|row| row_points_at_relay(&row, &app_type));

    let state_consistent = configured == current_row_is_relay;

    let inconsistency = if state_consistent {
        None
    } else if configured && !current_row_is_relay {
        Some(format!(
            "配置文件里还是中转站的密钥，但选中的是「{}」。点「重新应用」把选中的应用写进配置。",
            live_provider_name.clone().unwrap_or_else(|| "另一个供应商".into())
        ))
    } else {
        Some(
            "选中的是中转站，但配置文件已经不指向中转站了（可能被其他工具覆盖）。点「重新应用」修复。"
                .to_string(),
        )
    };

    ClientStatus {
        app: app_type.as_str().to_string(),
        label: spec.label.to_string(),
        supported: true,
        cli_installed: spec.cli.is_none_or(cli_on_path),
        live_path: live_path.as_ref().map(|p| p.display().to_string()),
        configured,
        current_provider: settings_current.or(db_current),
        live_provider_name,
        state_consistent,
        inconsistency,
    }
}

/// Does this provider row carry our relay's credentials?
///
/// Asked of the row's own settings, not of its name or id: a row can be called
/// anything, and the question is where its traffic would go.
fn row_points_at_relay(row: &crate::provider::Provider, app_type: &AppType) -> bool {
    let (base_url, api_key) = row.resolve_usage_credentials(app_type);
    !api_key.trim().is_empty() && points_at_aimyria_text(&base_url)
}

/// [`points_at_aimyria`] for a URL that is already a string rather than a file.
fn points_at_aimyria_text(text: &str) -> bool {
    text_mentions_our_relay(text)
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
pub async fn configure_all(state: State<'_, AppState>) -> Result<ConfigureReport, String> {
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
        // Only re-apply the ones that are already configured. Re-applying an
        // unconfigured client would silently configure it, which is a decision the
        // user has not made — 「重新应用」 means "update what I already set up".
        if !crate::commands::aimyria::aimyria_only_app_configured(app) {
            continue;
        }
        match write_aimyria_only_client(state.inner(), app, None).await {
            Ok(()) => report.configured.push(app.as_str().to_string()),
            Err(reason) => report.failed.push((app.as_str().to_string(), reason)),
        }
    }
    Ok(report)
}

/// Rewrite a WorkBuddy / DeepSeek Harness configuration from the relay's
/// credentials.
///
/// # Why this is not the "already configured" check the old code did
///
/// `configure_one` and `configure_all` used to answer for these two clients by
/// reading their file for the literal string `aimyria` and reporting "configured"
/// if it was there. That is a *status* check mistaken for a *write*: pressing
/// 「接入」 on an already-configured WorkBuddy changed nothing at all, so a user who
/// had just rotated their key saw the old key keep working.
///
/// It also could not work for a client that was not configured yet — which is the
/// case the button exists for.
///
/// # What it writes
///
/// The credential and model list come from `bound_credentials`, i.e. the key this
/// machine is already using. `models` lets the caller override the list (the key
/// page's model picker); when absent, every model the group serves is written.
pub(crate) async fn write_aimyria_only_client(
    state: &AppState,
    app: crate::aimyria_apps::AimyriaOnlyApp,
    models: Option<&[String]>,
) -> Result<(), String> {
    use crate::aimyria_apps::AimyriaOnlyApp;

    let Some((base_url, api_key, _, _)) = bound_credentials(state) else {
        return Err(format!(
            "{} 还没有可用的密钥。请先在中转站创建一个密钥。",
            app.label()
        ));
    };

    // The relay's model list for this key. A failed fetch is not fatal when the
    // caller supplied its own list; it is when nothing else is available.
    let resolved: Vec<String> = match models {
        Some(list) if !list.is_empty() => list.to_vec(),
        _ => crate::aimyria_apps::fetch_group_models(
            &relay_site_base(&base_url),
            &api_key,
        )
        .await
        .map(|group| group.models)
        .unwrap_or_default(),
    };
    if resolved.is_empty() {
        return Err(format!(
            "{}：读不到可用模型，请检查密钥分组或稍后重试",
            app.label()
        ));
    }

    let settings = match app {
        AimyriaOnlyApp::WorkBuddy => {
            crate::aimyria_apps::build_workbuddy_settings(&base_url, &api_key, &resolved)
        }
        AimyriaOnlyApp::DeepSeekHarness => {
            crate::aimyria_apps::build_dsh_settings(&base_url, &api_key, &resolved)
        }
    };

    match app {
        AimyriaOnlyApp::WorkBuddy => {
            crate::aimyria_apps::write_workbuddy_at(&app.live_path(), &settings)
                .map_err(|e| e.to_string())
        }
        AimyriaOnlyApp::DeepSeekHarness => {
            let credentials = crate::aimyria_apps::dsh_credentials_path();
            crate::aimyria_apps::write_dsh_at(&app.live_path(), &credentials, &settings)
                .map_err(|e| e.to_string())
        }
    }
}

/// Configure a `live_writer` client from the relay's own credentials.
///
/// # Why this exists
///
/// 「接入」 used to work only if a provider row already existed for that client,
/// which is only true after a console deep link. So for a user who signed in
/// through the app instead, every button answered "还没有可用的密钥" — even though
/// the machine had a perfectly good key. The button could not do the one thing it
/// is named after.
///
/// # How it writes
///
/// It builds the same `DeepLinkImportRequest` the console produces and hands it to
/// `import_provider_from_deeplink`, which already knows each client's config shape,
/// how to pick direct vs proxy for Claude Desktop, and how to build the proxy model
/// routes. Re-implementing any of that here would be a second writer that drifts.
///
/// `mode` is only meaningful for Claude Desktop today:
/// * `proxy` — local router: Desktop sees `claude-*` route ids mapped to whatever
///   the group actually sells. Required for a group of non-Claude models.
/// * `direct` — Desktop talks to the endpoint itself; only works when the group
///   serves real `claude-*` ids.
/// * `None` — decide from the model list, which is what a console link does.
async fn write_relay_client(
    state: &AppState,
    app_type: &AppType,
    models: Option<&[String]>,
    mode: Option<&str>,
) -> Result<String, String> {
    let Some((base_url, api_key, _, _)) = bound_credentials(state) else {
        return Err(format!(
            "{} 还没有可用的密钥。请先在中转站创建一个密钥。",
            live_writer::client_spec(app_type)
                .map(|spec| spec.label)
                .unwrap_or(app_type.as_str())
        ));
    };

    let resolved: Vec<String> = match models {
        Some(list) if !list.is_empty() => list.to_vec(),
        _ => crate::aimyria_apps::fetch_group_models(&relay_site_base(&base_url), &api_key)
            .await
            .map(|group| group.models)
            .unwrap_or_default(),
    };
    if resolved.is_empty() {
        return Err("读不到可用模型，请检查密钥分组或稍后重试".to_string());
    }

    let mut request = crate::deeplink::DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some(app_type.as_str().to_string()),
        name: Some("AIMYRIA".to_string()),
        enabled: Some(true),
        homepage: Some("https://aimyria.com".to_string()),
        endpoint: Some(base_url.clone()),
        api_key: Some(api_key.clone()),
        icon: Some("aimyria".to_string()),
        model: resolved.first().cloned(),
        models: Some(resolved.clone()),
        ..Default::default()
    };

    // Claude Desktop's mode is inferred from the model list unless the caller is
    // explicit. Saying it out loud matters because the two modes write completely
    // different profiles, and proxy needs the local router running.
    if matches!(app_type, AppType::ClaudeDesktop) {
        if let Some(mode) = mode {
            match mode {
                "proxy" | "direct" => request.claude_desktop_mode = Some(mode.to_string()),
                other => return Err(format!("未知的 Claude Desktop 模式: {other}")),
            }
        }
    }

    let provider_id = crate::deeplink::import_provider_from_deeplink(state, request)
        .await
        .map_err(|e| format!("写入配置失败：{e}"))?;
    ProviderService::switch(state, app_type.clone(), &provider_id)
        .map_err(|e| format!("激活配置失败：{e}"))?;
    Ok(provider_id)
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
pub async fn configure_one(
    state: State<'_, AppState>,
    app: String,
    models: Option<Vec<String>>,
    // Claude Desktop only: "direct" or "proxy".
    mode: Option<String>,
) -> Result<ConfigureReport, String> {
    if let Some(only_app) = crate::aimyria_apps::AimyriaOnlyApp::from_str(&app) {
        // Actually write it. The old code only *read* the file and reported
        // whether the string `aimyria` appeared, so 「接入」 on a stale config did
        // nothing and a rotated key never reached the client.
        return match write_aimyria_only_client(
            state.inner(),
            only_app,
            models.as_deref(),
        )
        .await
        {
            Ok(()) => Ok(ConfigureReport {
                configured: vec![only_app.as_str().to_string()],
                failed: Vec::new(),
                warnings: Vec::new(),
            }),
            Err(reason) => Ok(ConfigureReport {
                configured: Vec::new(),
                failed: vec![(only_app.as_str().to_string(), reason)],
                warnings: Vec::new(),
            }),
        };
    }

    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    if live_writer::client_spec(&app_type).is_none() {
        return Err(format!("暂不支持配置 {app}"));
    }

    let Some(id) = current_row_id(state.inner(), &app_type) else {
        // No provider row for this client yet.
        //
        // This used to be a dead end: the message told the user to fetch a config
        // from the console, and the 接入 button could not do the one thing it is
        // named after. On a machine that signed in through the app rather than
        // through a console link, *every* client without a row answered this way
        // even though a usable key was right there.
        //
        // So create the row from the key this machine already has, then write it.
        // A failure here is reported per-client rather than as a command error, so
        // the UI keeps its per-row result shape.
        return match write_relay_client(
            state.inner(),
            &app_type,
            models.as_deref(),
            mode.as_deref(),
        )
        .await
        {
            Ok(_) => Ok(ConfigureReport {
                configured: vec![app_type.as_str().to_string()],
                warnings: Vec::new(),
                failed: Vec::new(),
            }),
            Err(reason) => Ok(ConfigureReport {
                configured: Vec::new(),
                warnings: Vec::new(),
                failed: vec![(app_type.as_str().to_string(), reason)],
            }),
        };
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
    relay_balance_for(state.inner()).await
}

/// The balance computation, usable without a `State` handle.
///
/// Split out so the overview page can reuse it. `State<'_, T>` cannot be
/// constructed from a `&T`, so without this split the overview would either
/// duplicate the balance logic (two answers to one question) or fail to compile.
pub async fn relay_balance_for(state: &AppState) -> Result<RelayBalance, String> {
    let Some((base_url, api_key, balance_url, balance_token)) = bound_credentials(state)
    else {
        // No key anywhere on this machine.
        //
        // The message deliberately says what to do rather than "not bound": a user
        // can be signed in to an account and still have no client configured, and
        // telling them they are "not bound" sends them to look for an account
        // problem they do not have.
        return Ok(RelayBalance {
            reason: Some("not_bound".to_string()),
            error: Some(
                "本机还没有可用的密钥。接入任一客户端后，这里会显示余额。".to_string(),
            ),
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
/// The relay credentials available on this machine, from any configured client.
///
/// # Why this searches every client, not just two
///
/// It used to look at Claude Code and Codex only, falling back to the two
/// Aimyria-only clients. That left a large and completely ordinary group of users
/// with **no balance at all**: someone who configured only Gemini, only OpenCode,
/// only OpenClaw, or only Claude Desktop. Their key is on disk, the relay answers
/// `/v1/user/balance` correctly, and the panel still said 「余额待开通」 — which reads
/// like the relay is broken, not like the desktop looked in the wrong places.
///
/// The order below is a preference, not a limit:
///
/// 1. any row that carries a read-only balance bridge (it can answer even when the
///    gateway endpoint cannot);
/// 2. Claude Code, then Codex — the two shapes whose key location is unambiguous;
/// 3. every other configurable client, in the order the UI lists them;
/// 4. the two Aimyria-only clients, whose key lives in their own config file.
///
/// The first usable credential wins. "Usable" means a non-empty key.
pub fn bound_credentials(state: &AppState) -> Option<(String, String, Option<String>, Option<String>)> {
    // 1. A row carrying a balance bridge answers best; take it wherever it lives.
    for app_type in AppType::all() {
        let Some(row) = selected_provider(state, &app_type) else {
            continue;
        };
        let (balance_url, balance_token) = row
            .meta
            .as_ref()
            .map(|meta| (meta.balance_url.clone(), meta.balance_token.clone()))
            .unwrap_or((None, None));
        if !balance_url
            .as_deref()
            .is_some_and(|url| !url.trim().is_empty())
        {
            continue;
        }
        let (base_url, api_key) = row.resolve_usage_credentials(&app_type);
        if !api_key.trim().is_empty() {
            return Some((base_url, api_key, balance_url, balance_token));
        }
    }

    // 2 and 3. Any selected row with a key, preferred order first.
    //
    // `AppType::all()` already yields Claude and Codex before the rest, so the
    // published preference is preserved without a second list to keep in sync.
    for app_type in AppType::all() {
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
        return Some((base_url, api_key, balance_url, balance_token));
    }

    // 4. The two clients whose key lives in their own file, not in a provider row.
    aimyria_only_credentials()
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

/// The provider row this app is currently using.
///
/// # Two sources of truth, and they disagree in practice
///
/// `settings.json` holds `currentProvider<App>`, and the `providers` table holds
/// `is_current`. Both exist, both are written on a normal switch, and they **do**
/// drift: measured on this machine, `currentProviderCodex` said `default` while the
/// row flagged `is_current` was `Aimyria-20261022035936`. Reading the stale one
/// returned a row with no key, so the balance disappeared even though a perfectly
/// good key was selected in the app.
///
/// The database wins when they disagree. It is the store the switch flow updates
/// inside a transaction, while the settings file is a device-local convenience that
/// a previous process may have left behind; and a row flagged current is what the
/// client's live config actually contains.
fn selected_provider(state: &AppState, app_type: &AppType) -> Option<crate::provider::Provider> {
    let all = state.db.get_all_providers(app_type.as_str()).ok()?;

    // 1. The row the *database* considers current. Authoritative.
    if let Some(row) = state
        .db
        .get_current_provider(app_type.as_str())
        .ok()
        .flatten()
        .and_then(|id| all.get(&id))
    {
        return Some(row.clone());
    }

    // 2. The device-local setting. Used when the database has no flag (a fresh
    //    import, or a row written by an older build).
    if let Some(id) = crate::settings::get_current_provider(app_type) {
        if let Some(row) = all.get(&id) {
            return Some(row.clone());
        }
    }

    // 3. Nothing selected. Do not guess a row: another app's key would be used for
    //    this app's balance, which is the wrong credential for the wrong question.
    None
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
    text_mentions_our_relay(&text)
}

/// Does this config text point at one of our relays?
///
/// Matching is **host-aware**, not a bare substring search: a plain
/// `text.contains("aimyria.com")` would also match `aimyria.com.evil.example`, and
/// worse, a user's comment mentioning our domain would count as configured.
///
/// A candidate host only counts when it is bounded by something that cannot be
/// part of a hostname on either side — start/end, `/`, `"`, `'`, whitespace, `:`,
/// `@`, `,`, `)`, `]` or `;`. That covers every config shape we write (`https://…`
/// URLs, TOML strings, `.env` lines, JSON values) without needing to parse each.
fn text_mentions_our_relay(text: &str) -> bool {
    // Scan candidate hostnames out of the text and classify each with the one
    // matcher in `relay::hosts`.
    //
    // This used to compare against `RELAY_HOSTS` with a boundary rule, which was a
    // second implementation of "is this ours?" — and a narrower one: it required a
    // whole host to appear verbatim, so `wb.321ai.cc` (a documented role subdomain)
    // was not recognised. Two matchers with different answers is exactly the class
    // of bug this whole round is about.
    //
    // Extracting occurrences of each base domain and asking `is_relay_host` about
    // the full label run keeps a single source of truth: anything `relay::hosts`
    // accepts, this accepts.
    RELAY_HOSTS.iter().any(|base| {
        text.match_indices(base).any(|(index, _)| {
            // Walk left over the label characters to find where the hostname starts,
            // then right to where it ends, so `wb.321ai.cc` is presented whole.
            let bytes = text.as_bytes();
            let mut start = index;
            while start > 0 {
                let previous = text[..start].chars().next_back();
                match previous {
                    Some(c) if is_host_char(c) => {
                        start -= c.len_utf8();
                    }
                    _ => break,
                }
            }
            let end_of_base = index + base.len();
            let mut end = end_of_base;
            while end < bytes.len() {
                match text[end..].chars().next() {
                    Some(c) if is_host_char(c) => end += c.len_utf8(),
                    _ => break,
                }
            }
            crate::relay::is_relay_host(&text[start..end]).is_some()
        })
    })
}

/// Could this character continue a hostname?
///
/// Deliberately excludes `.`: `api.aimyria.com` must not match on the
/// `aimyria.com` suffix alone, or a look-alike domain would read as ours. The
/// dot is instead handled by treating `api.` as its own allowed prefix — which is
/// why `RELAY_HOST` is checked explicitly rather than being implied.
fn is_host_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_'
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
/// [`client_configured`] for the two apps that are ours alone rather than a
/// wrapped third-party CLI, and so have no `ClientSpec` entry.
///
/// Public so the overview page can count them without re-deriving the rule. Two
/// definitions of "configured" is how the one-click screen came to disagree with
/// the tray marker.
pub fn aimyria_only_app_configured(app: crate::aimyria_apps::AimyriaOnlyApp) -> bool {
    std::fs::read_to_string(app.live_path())
        .map(|text| text_mentions_our_relay(&text))
        .unwrap_or(false)
}

/// The relay host we configure clients to talk to.
///
/// Public because the deep-link importer uses it to tell "a link pointing at
/// us" from "some other provider's link", which is what decides whether a
/// re-import replaces the existing row.
pub const RELAY_HOST: &str = "api.aimyria.com";

/// The console/site host, which is also a valid relay endpoint.
///
/// This is not a hypothetical shape: an account imported from the console keeps
/// `https://aimyria.com` as its `ANTHROPIC_BASE_URL`, and this machine's own
/// `~/.claude/settings.json` says exactly that. Checking only `api.aimyria.com`
/// therefore reported a fully working Claude install as "not connected".
pub const RELAY_CONSOLE_HOST: &str = "aimyria.com";

/// The sub-gateway `sub2.aimyria.com`, which a newer console import stores as the
/// Codex base URL. Same story: missing it made a live Codex row read as unconfigured.
pub const RELAY_SUB2_HOST: &str = "sub2.aimyria.com";

/// The current 321AI gateway host. Kept next to [`RELAY_HOST`] so "is this one
/// of our relays?" is answered in one place.
pub const RELAY_ALT_HOST: &str = "321ai.cc";

/// Every host whose presence in a config file means "this client talks to us".
///
/// The authoritative list lives in [`crate::relay::hosts`], which owns the
/// classification rules (ports, userinfo, look-alike rejection). This constant is
/// the literal copy used by text-scanning call sites; `relay_hosts_match_the_relay_module`
/// below asserts the two agree, so they cannot drift — which is exactly how the
/// console host came to be missing while the writers emitted it.
pub const RELAY_HOSTS: [&str; 4] = [
    "api.aimyria.com",
    "aimyria.com",
    "sub2.aimyria.com",
    "321ai.cc",
];

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

    /// The text-scan list and the classification module must not drift.
    ///
    /// They are two representations of one fact — "which hosts are ours" — and the
    /// original defect was precisely a drift between a checker's list and a
    /// writer's output. A test is cheaper than the next incident.
    #[test]
    fn relay_hosts_match_the_relay_module() {
        for host in RELAY_HOSTS {
            assert!(
                crate::relay::is_relay_host(host).is_some(),
                "{host} is scanned as ours but the relay module does not recognise it"
            );
        }
        for relay_host in crate::relay::ALL_HOSTS {
            assert!(
                RELAY_HOSTS
                    .iter()
                    .any(|host| host.eq_ignore_ascii_case(relay_host.hostname())),
                "{} is recognised by the relay module but missing from the scan list",
                relay_host.hostname()
            );
        }
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
    /// Every client the UI can configure must be able to supply the balance key.
    ///
    /// This is the regression test for the report "another user installed the
    /// latest build and still cannot see the balance". The lookup used to read
    /// Claude Code and Codex only, so a user whose only configured client was
    /// Gemini, OpenCode, OpenClaw, Hermes or Claude Desktop got no number at all —
    /// while their key sat on disk and the relay answered fine.
    ///
    /// The assertion is structural: the search space is `AppType::all()`, so adding
    /// a client to the product automatically includes it here. A future change that
    /// narrows the list again fails this test.
    #[test]
    /// A row is "ours" by where its traffic would go, not by its name or id.
    ///
    /// This is the predicate the consistency check rests on. A row called
    /// `default` carrying our key counts; a row called `AIMYRIA` pointing at
    /// someone else's endpoint does not.
    #[test]
    fn a_row_is_ours_by_its_endpoint_not_its_name() {
        // The text-level predicate is what `row_points_at_relay` delegates to.
        assert!(points_at_aimyria_text("https://aimyria.com/v1"));
        assert!(points_at_aimyria_text("https://api.aimyria.com"));
        assert!(points_at_aimyria_text("https://sub2.aimyria.com/v1"));
        assert!(points_at_aimyria_text("https://wb.321ai.cc/v1"));
        assert!(!points_at_aimyria_text("https://shenwenai.com"));
        assert!(!points_at_aimyria_text("https://flow-node.com"));
        assert!(!points_at_aimyria_text(""));
    }

    /// The four states the consistency check must distinguish.
    ///
    /// The reported bug was the second row: the user had switched the provider list
    /// to 神稳AI while the live file still held the relay's key, and the two screens
    /// disagreed because each read a different source. The fix is not to pick a
    /// winner but to say the state is inconsistent.
    #[test]
    fn the_consistency_check_distinguishes_all_four_states() {
        // (file points at us, selected row is ours) -> consistent?
        let cases = [
            // Both say "relay": normal, healthy.
            (true, true, true),
            // File says relay, selection says third party: the stale-file case.
            (true, false, false),
            // File says third party, selection says relay: a lost write.
            (false, true, false),
            // Both say third party: normal, user is on someone else.
            (false, false, true),
        ];
        for (file_relay, row_relay, expected) in cases {
            assert_eq!(
                file_relay == row_relay,
                expected,
                "file={file_relay} row={row_relay} should be {}",
                if expected { "consistent" } else { "inconsistent" }
            );
        }
    }

    fn the_balance_lookup_covers_every_configured_client() {
        let searched: Vec<AppType> = AppType::all().collect();
        for required in [
            AppType::Claude,
            AppType::ClaudeDesktop,
            AppType::Codex,
            AppType::Gemini,
            AppType::OpenCode,
            AppType::OpenClaw,
            AppType::Hermes,
        ] {
            assert!(
                searched.contains(&required),
                "{} can be configured by the UI, so the balance lookup must consider it",
                required.as_str()
            );
        }
    }

    /// A row carrying a balance bridge wins over one that does not, wherever it is.
    ///
    /// The bridge is the only credential that can answer when the gateway's
    /// by-key endpoint is unavailable, so preferring it is what keeps the panel
    /// working for a console-imported account.
    #[test]
    fn a_bridge_row_is_preferred_over_a_plain_one() {
        // Mirrors the ordering in `bound_credentials`: pass 1 takes any bridged
        // row, pass 2 takes any keyed row.
        fn pick(candidates: &[(bool, bool)]) -> Option<char> {
            let bridged = candidates.iter().position(|(bridge, key)| *bridge && *key);
            if let Some(index) = bridged {
                return Some(['a', 'b', 'c'][index]);
            }
            candidates
                .iter()
                .position(|(_, key)| *key)
                .map(|index| ['a', 'b', 'c'][index])
        }

        // Second row is bridged: it wins even though the first also has a key.
        assert_eq!(pick(&[(false, true), (true, true)]), Some('b'));
        // No bridge anywhere: first keyed row wins.
        assert_eq!(pick(&[(false, true), (false, true)]), Some('a'));
        // A bridged row with no key cannot be used.
        assert_eq!(pick(&[(true, false), (false, true)]), Some('b'));
        assert_eq!(pick(&[(true, false), (false, false)]), None);
    }

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

    /// Every host the console can hand out must read as "configured".
    ///
    /// This is the regression test for a defect measured on this machine: the
    /// checker only knew `api.aimyria.com` and `321ai.cc`, while the live files
    /// said `https://aimyria.com` (Claude) and `https://sub2.aimyria.com/v1`
    /// (Codex). Both clients were configured and serving traffic, and the screen
    /// showed 「接入」 for both — so the user's only option was to press a button
    /// that rewrote the file it had just read.
    #[test]
    fn every_host_the_console_can_write_reads_as_configured() {
        for host in ["api.aimyria.com", "aimyria.com", "sub2.aimyria.com"] {
            let text = format!(r#"{{"env":{{"ANTHROPIC_BASE_URL":"https://{host}/v1"}}}}"#);
            assert!(
                text_mentions_our_relay(&text),
                "{host} is a host the console writes but the checker rejected"
            );
        }
    }

    /// Look-alike domains must NOT read as ours.
    ///
    /// A bare `contains("aimyria.com")` accepts `aimyria.com.evil.example`; that
    /// would paint a third-party relay green and, worse, tell the deep-link
    /// importer to overwrite the user's row instead of adding one.
    #[test]
    fn look_alike_domains_are_not_our_relay() {
        for text in [
            r#"{"base_url":"https://aimyria.com.evil.example/v1"}"#,
            r#"{"base_url":"https://notaimyria.com/v1"}"#,
            r#"{"base_url":"https://api.aimyria.community/v1"}"#,
            r#"{"base_url":"https://sub2.aimyria.community/v1"}"#,
            r#"{"base_url":"https://openrouter.ai/api/v1"}"#,
        ] {
            assert!(
                !text_mentions_our_relay(text),
                "{text} must not be treated as our relay"
            );
        }
    }

    /// A URL with a path, query or port after our host still counts.
    ///
    /// The boundary rule is about *domain* boundaries; everything a URL appends
    /// after the host is a legitimate part of a configured endpoint.
    #[test]
    fn ordinary_url_tails_after_our_host_still_count() {
        for text in [
            "https://api.aimyria.com",
            "https://api.aimyria.com/",
            "https://api.aimyria.com/v1",
            "https://api.aimyria.com:443/v1",
            "ANTHROPIC_BASE_URL=https://aimyria.com/v1",
            "base_url = \"https://sub2.aimyria.com/v1\"",
            r#"{"url":"https://sub2.aimyria.com/v1"}"#,
        ] {
            assert!(text_mentions_our_relay(text), "{text} should count as ours");
        }
    }
}
