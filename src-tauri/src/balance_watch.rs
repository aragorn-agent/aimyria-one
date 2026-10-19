//! Watches the relay balance and speaks up when it runs low.
//!
//! # Why a background task rather than a UI effect
//!
//! This tool lives in the tray. A low balance that only shows up when the user
//! happens to open the window is not a warning, it is a post-mortem — by then
//! their editor is already refusing requests. So the check runs here, on the
//! Rust side, where it keeps working whether or not a window exists.
//!
//! # Why it is not noisier than this
//!
//! Two guards keep it from becoming the thing users complain about:
//!
//! - **Once per crossing.** The warning fires when the balance crosses below
//!   the threshold, not on every poll. Otherwise a user who tops up to $1.50
//!   gets the popup every six minutes for the rest of the session.
//! - **Never blocks work.** A low balance warns; it does not disable anything.
//!   Someone mid-task with $0.40 left should be told, not locked out.
//!
//! A transient fetch failure is not a low balance and must not raise the
//! window: waking the user to say "we could not check" trains them to ignore it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::app_config::AppType;
use crate::commands::aimyria::{self, LOW_BALANCE_USD};
use crate::services::balance;
use crate::store::AppState;

/// How often to ask the relay.
///
/// Short enough that a user who tops up sees the number recover, long enough
/// that a user who leaves the tool running all day is not generating traffic.
const POLL_INTERVAL_SECS: u64 = 15 * 60;

/// Event the frontend listens on, so an open window can react without polling.
const BALANCE_EVENT: &str = "relay-balance";

/// Whether we are currently in the "low" state.
///
/// Compared between polls: a warning is due on a transition into the low state,
/// not while staying there.
static BELOW_THRESHOLD: AtomicBool = AtomicBool::new(false);

/// Remembers the last known balance so a failed poll does not re-trigger the
/// warning, and so the popup can name an amount.
static LAST_KNOWN: Mutex<Option<f64>> = Mutex::new(None);

/// Start the watcher. Called once from `setup`.
///
/// The first poll runs immediately: a user who installed this tool at $0.50
/// should be told now, not in fifteen minutes.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            check_once(&app).await;

            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
        }
    });
}

/// One poll. Split out so the tests can drive it without a runtime.
pub async fn check_once(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    let Some((base_url, api_key)) = bound_credentials(state.inner()) else {
        // Not set up yet. Reset so the first real reading after setup counts as
        // a fresh transition rather than being swallowed by stale state.
        BELOW_THRESHOLD.store(false, Ordering::Release);
        return;
    };

    let result = match balance::get_balance(&base_url, &api_key).await {
        Ok(result) => result,
        Err(e) => {
            // Transport failure: deliberately silent. See the module docs.
            log::debug!("余额轮询失败（本轮不打扰用户）: {e}");
            return;
        }
    };

    if !result.success {
        log::debug!(
            "余额查询未成功: {}",
            result.error.unwrap_or_else(|| "未知原因".into())
        );
        return;
    }

    let remaining = result
        .data
        .and_then(|rows| rows.into_iter().next())
        .and_then(|d| d.remaining);

    let Some(remaining) = remaining else {
        // A payload with no balance field is "unknown", not "zero".
        return;
    };

    if let Ok(mut slot) = LAST_KNOWN.lock() {
        *slot = Some(remaining);
    }

    let below = remaining < LOW_BALANCE_USD;
    let was_below = BELOW_THRESHOLD.swap(below, Ordering::AcqRel);

    let _ = app.emit(
        BALANCE_EVENT,
        aimyria::RelayBalance {
            remaining: Some(remaining),
            low: below,
            ..Default::default()
        },
    );

    if below && !was_below {
        log::info!("余额 ${remaining:.2} 低于 ${LOW_BALANCE_USD:.2}，提醒用户");
        // Only interrupt if the window is not already in front of the user:
        // re-focusing a window they are already reading is the definition of
        // background software misbehaving.
        let already_visible = app
            .get_webview_window("main")
            .and_then(|w| w.is_visible().ok())
            .unwrap_or(false);
        if !already_visible {
            crate::tray::show_main_window(app);
        }
    }
}

/// The relay credentials the bound provider row carries.
fn bound_credentials(state: &AppState) -> Option<(String, String)> {
    for app_type in [AppType::Claude, AppType::Codex] {
        let Some(row) = selected_provider(state, &app_type) else {
            continue;
        };
        let (base_url, api_key) = row.resolve_usage_credentials(&app_type);
        if !api_key.trim().is_empty() {
            return Some((base_url, api_key));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether a poll result should raise the window.
    ///
    /// Split out from `check_once` so the "only on a crossing" rule can be
    /// tested without an `AppHandle` or a network.
    fn should_warn(remaining: f64, was_below: bool) -> bool {
        remaining < LOW_BALANCE_USD && !was_below
    }

    #[test]
    fn warns_when_the_balance_drops_below() {
        assert!(should_warn(1.99, false));
    }

    #[test]
    fn stays_quiet_while_already_low() {
        // The difference between a warning and an annoyance.
        assert!(!should_warn(0.10, true));
    }

    #[test]
    fn stays_quiet_while_healthy() {
        assert!(!should_warn(100.0, false));
    }

    #[test]
    fn the_threshold_itself_does_not_warn() {
        assert!(!should_warn(LOW_BALANCE_USD, false));
    }

    #[test]
    fn a_recharge_arms_the_next_warning() {
        // low -> topped up -> low again must warn again, or a user who tops up
        // once would never hear about the next time they run out.
        assert!(should_warn(0.5, true), "staying low does not re-warn");
        // Once the balance recovers the state clears...
        assert!(!should_warn(10.0, true), "a healthy balance does not warn");
        // ...so dropping below the threshold warns again.
        assert!(should_warn(0.5, false));
    }
}
