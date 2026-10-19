//! Applying a parsed deep link: this is what "一键配置" actually means.
//!
//! # The whole point
//!
//! A user clicks a button in a browser. This module is the path after the
//! confirmation dialog: parse, store the key, write the selected client's
//! config file. The dialog exists because the URL is untrusted input; see the
//! safety note on [`apply_deeplink_request`].
//!
//! # Why it is not in the renderer
//!
//! It was, upstream: the Rust side parsed the URL, emitted an event, and React
//! called back into `import_from_deeplink_unified`. This app runs with its
//! window hidden, so on a real install there was no React app mounted to receive
//! that event and the click did nothing. The product's central promise failed
//! silently, for every user, always.
//!
//! Doing it here means the outcome does not depend on whether a webview has
//! finished loading, whether it is visible, or whether the frontend bundle even
//! has a bug.
//!
//! # What the user is told
//!
//! Success writes the file and logs it. Failure shows the window with an error,
//! because "I clicked a button in a browser and nothing happened" is the worst
//! possible outcome and it must not be silent.

use std::str::FromStr;

use tauri::{AppHandle, Manager};

use crate::deeplink::{import_provider_from_deeplink, DeepLinkImportRequest};
use crate::error::AppError;
use crate::services::provider::ProviderService;
use crate::store::AppState;

/// Apply a deep link to exactly the client the user chose.
///
/// # Why one client, not all of them
///
/// The website asks the user which app to import into, then sends `app=claude`
/// (or `claude-desktop`, `codex`, `workbuddy`, `dsh`, `hermes`, …). Writing every
/// client anyway defeats that choice: the user sees one app confirmed, and four
/// more silently point at the relay. The old "configure all five" behaviour was
/// a workaround for a link that carried no choice; the chooser now carries it.
///
/// # Why the write happens here and not in the renderer
///
/// This app runs in the tray. A deep link can arrive before any React tree is
/// mounted, so the import must not depend on the webview. The UI receives an
/// event afterwards only to refresh what it displays.
///
/// # Safety
///
/// The caller must have shown the confirmation dialog before calling this.
/// `handle_deeplink_url` therefore stashes the parsed request and emits it to
/// the frontend instead of calling this function directly.
pub async fn apply_deeplink_request(
    app: &AppHandle,
    request: &DeepLinkImportRequest,
) -> Result<String, AppError> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| AppError::Message("应用尚未就绪".to_string()))?;

    let named = request
        .app
        .clone()
        .ok_or_else(|| AppError::InvalidInput("deep link 缺少 app 参数".to_string()))?;

    let models = request.models.clone().unwrap_or_default();
    let group_name = request.group_name.clone().unwrap_or_default();

    if let Some(only_app) = crate::aimyria_apps::AimyriaOnlyApp::from_str(&named) {
        let settings =
            crate::deeplink::provider::build_aimyria_only_settings(only_app.as_str(), &request)?;
        match only_app {
            crate::aimyria_apps::AimyriaOnlyApp::WorkBuddy => {
                crate::aimyria_apps::write_workbuddy(&settings)?;
            }
            crate::aimyria_apps::AimyriaOnlyApp::DeepSeekHarness => {
                crate::aimyria_apps::write_dsh(&settings)?;
            }
        }
        log::info!(
            "Aimyria-only client {} configured at {} (group {}, {} models)",
            only_app.as_str(),
            only_app.live_path().display(),
            if group_name.is_empty() {
                "-"
            } else {
                &group_name
            },
            models.len()
        );
        return Ok(format!(
            "已配置 {}（{}）",
            only_app.label(),
            only_app.live_path().display()
        ));
    }

    let app_type = crate::app_config::AppType::from_str(&named)
        .map_err(|e| AppError::InvalidInput(format!("不支持的客户端: {named}（{e}）")))?;
    force_direct_before_switch(state.inner(), &app_type);

    let mut for_client = request.clone();
    for_client.app = Some(app_type.as_str().to_string());
    let provider_id =
        crate::deeplink::import_provider_from_deeplink(state.inner(), for_client).await?;
    // The website sends a short-lived balance bridge on the same link. Persist
    // it on the row that was just created, so the balance panel keeps working
    // after the confirmation dialog closes.
    let _ = persist_balance_bridge(state.inner(), &app_type, &request);
    let result = ProviderService::switch(state.inner(), app_type.clone(), &provider_id)?;

    let mut summary = format!("已配置 {}（供应商 {provider_id}）", app_type.as_str());
    if !result.warnings.is_empty() {
        summary.push_str(&format!("，警告：{}", result.warnings.join("；")));
    }
    crate::tray::refresh_tray_if_problems_changed(app);
    Ok(summary)
}

fn persist_balance_bridge(
    state: &AppState,
    app_type: &crate::app_config::AppType,
    request: &DeepLinkImportRequest,
) -> Result<(), AppError> {
    let Some(mut provider) = state
        .db
        .get_provider_by_id(crate::live_writer::PROVIDER_ID, app_type.as_str())?
    else {
        return Ok(());
    };
    if request.balance_url.is_none() && request.balance_token.is_none() {
        return Ok(());
    }
    let meta = provider
        .meta
        .get_or_insert_with(crate::provider::ProviderMeta::default);
    meta.balance_url = request.balance_url.clone();
    meta.balance_token = request.balance_token.clone();
    state.db.save_provider(app_type.as_str(), &provider)
}

/// Put an app back into direct mode before a deep link switches it.
fn force_direct_before_switch(
    state: &crate::store::AppState,
    app_type: &crate::app_config::AppType,
) {
    if !crate::mode::current::is_proxy(app_type) {
        return;
    }
    if let Err(e) = futures::executor::block_on(crate::mode::controller::exit(state, app_type)) {
        log::warn!("深链切换前退出 {} 的路由模式失败: {e}", app_type.as_str());
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Parse rather than hand-build: the request type has grown fields over
    /// upstream's life, and a literal here would rot the next time one is added
    /// — or worse, keep compiling while asserting about a stale shape.
    fn parse(url: &str) -> Result<DeepLinkImportRequest, crate::error::AppError> {
        crate::deeplink::parse_deeplink_url(url)
    }

    const GOOD: &str = "aimyria://v1/import?resource=provider&app=claude&name=Aimyria\
                        &endpoint=https://api.aimyria.com&apiKey=sk-test";

    #[test]
    fn a_well_formed_provider_link_parses() {
        let parsed = parse(GOOD).expect("a well-formed link must parse");
        assert_eq!(parsed.resource, "provider");
        assert_eq!(parsed.app.as_deref(), Some("claude"));
        assert_eq!(parsed.api_key.as_deref(), Some("sk-test"));
        assert_eq!(parsed.endpoint.as_deref(), Some("https://api.aimyria.com"));
    }

    /// The app name is validated during parsing, so a link naming a client we
    /// do not support cannot reach the writer at all.
    #[test]
    fn an_unsupported_app_is_rejected_at_parse_time() {
        assert!(
            parse(
                "aimyria://v1/import?resource=provider&app=nope&name=x&endpoint=https://a.example"
            )
            .is_err(),
            "an unknown app must not parse",
        );
    }

    #[test]
    fn a_provider_link_without_a_key_is_rejected() {
        assert!(
            parse("aimyria://v1/import?resource=provider&app=claude&name=x&endpoint=https://a.example")
                .is_err(),
            "a provider link with no key must not parse",
        );
    }

    /// Every client the product claims to configure must be accepted by the
    /// parser. A client we advertise but the link format rejects is a client
    /// whose 一键配置 silently does nothing.
    #[test]
    fn every_supported_client_is_accepted_by_the_link_format() {
        for app in [
            "claude",
            "claude-desktop",
            "codex",
            "gemini",
            "opencode",
            "openclaw",
            "hermes",
            "workbuddy",
            "dsh",
        ] {
            let url = format!(
                "aimyria://v1/import?resource=provider&app={app}&name=Aimyria\
                 &endpoint=https://api.aimyria.com&apiKey=sk-test"
            );
            let parsed =
                parse(&url).unwrap_or_else(|e| panic!("{app} should be accepted, got: {e}"));
            assert_eq!(parsed.app.as_deref(), Some(app));
        }
    }
}
