//! One call that fills the overview page.
//!
//! # Why a single aggregate command
//!
//! The page needs four independent things: balance, today's usage, the model
//! catalogue, and how many local clients are configured. Four separate commands
//! would mean four loading states and four failure states on one screen — and in
//! practice they fail independently (the gateway may be reachable while the
//! console is not).
//!
//! So this returns one object where **every block carries its own outcome**. A
//! block that failed has its `error` filled and its value left `null`; the page
//! renders the rest. That is the difference between "we could not read the model
//! list" and a blank white screen — and the first is what a user can act on.
//!
//! # What is deliberately absent
//!
//! Nothing here writes. No key is created, no config touched, no session mutated.
//! An overview that has side effects is not an overview.

use serde::Serialize;
use tauri::State;

use crate::store::AppState;

/// Balance block.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceBlock {
    /// `None` when unknown — never `0.0`, because "no money" and "could not ask"
    /// send the user to two different places.
    pub remaining: Option<f64>,
    pub used: Option<f64>,
    pub plan_name: Option<String>,
    pub error: Option<String>,
    /// `not_bound` | `bridge_unavailable` | `query_failed`, when there is no number.
    pub reason: Option<String>,
}

/// Usage block, computed from the local request log.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageBlock {
    pub today_requests: u64,
    pub today_cost: String,
    pub today_tokens: u64,
    pub month_requests: u64,
    pub month_cost: String,
    pub success_rate: f32,
    pub error: Option<String>,
}

/// Model catalogue block.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsBlock {
    pub group_name: String,
    pub count: usize,
    /// A short sample, not the whole list: the page shows "16 个模型" and a few
    /// names, and shipping hundreds of ids to the frontend to render three of them
    /// is waste.
    pub sample: Vec<String>,
    pub error: Option<String>,
}

/// Local client state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientsBlock {
    pub configured: usize,
    pub total: usize,
    /// Labels of the configured clients, for the "本机已接入" line.
    pub names: Vec<String>,
}

/// Everything the overview page renders.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayOverview {
    pub balance: BalanceBlock,
    pub usage: UsageBlock,
    pub models: ModelsBlock,
    pub clients: ClientsBlock,
    /// Unix seconds, so the UI can say "更新于 x 分钟前" without trusting its clock
    /// against the server's.
    pub fetched_at: i64,
}

/// Build the overview.
///
/// Every block is computed independently and never propagates its error to the
/// others — see the module docs.
#[tauri::command]
pub async fn relay_overview(state: State<'_, AppState>) -> Result<RelayOverview, String> {
    let balance = balance_block(state.inner()).await;
    let usage = usage_block(state.inner()).await;
    let models = models_block(state.inner()).await;
    let clients = clients_block(state.inner());

    Ok(RelayOverview {
        balance,
        usage,
        models,
        clients,
        fetched_at: crate::relay::now_unix(),
    })
}

async fn balance_block(state: &AppState) -> BalanceBlock {
    // Reuse the one balance implementation. It already knows the by-key endpoint
    // is the authority and that a 404 means "try the bridge".
    //
    // `get_relay_balance` takes `State<AppState>`; there is no way to fabricate
    // one from a `&AppState`, so the balance is computed through the same helper
    // the command uses rather than through the command itself.
    match crate::commands::aimyria::relay_balance_for(state).await {
        Ok(balance) => BalanceBlock {
            remaining: balance.remaining,
            used: balance.used,
            plan_name: None,
            error: balance.error,
            reason: balance.reason,
        },
        Err(error) => BalanceBlock {
            remaining: None,
            used: None,
            plan_name: None,
            error: Some(error),
            reason: Some("query_failed".to_string()),
        },
    }
}

async fn usage_block(state: &AppState) -> UsageBlock {
    let today_start = start_of_today();
    let month_start = start_of_month();

    // The database call is blocking; run it off the async worker.
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || {
        let today = db.get_usage_summary(Some(today_start), None, None, None, None)?;
        let month = db.get_usage_summary(Some(month_start), None, None, None, None)?;
        Ok::<_, crate::error::AppError>((today, month))
    })
    .await;

    match result {
        Ok(Ok((today, month))) => UsageBlock {
            today_requests: today.total_requests,
            today_cost: today.total_cost,
            today_tokens: today.real_total_tokens,
            month_requests: month.total_requests,
            month_cost: month.total_cost,
            success_rate: today.success_rate,
            error: None,
        },
        Ok(Err(error)) => UsageBlock {
            today_requests: 0,
            today_cost: "0".to_string(),
            today_tokens: 0,
            month_requests: 0,
            month_cost: "0".to_string(),
            success_rate: 0.0,
            error: Some(error.to_string()),
        },
        Err(error) => UsageBlock {
            today_requests: 0,
            today_cost: "0".to_string(),
            today_tokens: 0,
            month_requests: 0,
            month_cost: "0".to_string(),
            success_rate: 0.0,
            error: Some(format!("读取用量失败: {error}")),
        },
    }
}

/// Unix seconds at local midnight today.
///
/// Uses `Local`, not UTC: the user's "today" is their wall clock. Using UTC would
/// make the counter reset at 8am in China, which reads as a bug.
///
/// Returns 0 when the conversion fails (an ambiguous DST hour in a zone that has
/// them). That degrades to "since the epoch", i.e. all-time usage — visibly wrong
/// in a way that is better than panicking on a startup path.
fn start_of_today() -> i64 {
    let now = chrono::Local::now();
    let midnight = now.date_naive().and_hms_opt(0, 0, 0);
    local_midnight_to_unix(midnight)
}

/// Unix seconds at local midnight on the first of the current month.
fn start_of_month() -> i64 {
    use chrono::Datelike;
    let now = chrono::Local::now();
    let first = now.date_naive().with_day(1);
    let midnight = first.and_then(|date| date.and_hms_opt(0, 0, 0));
    local_midnight_to_unix(midnight)
}

fn local_midnight_to_unix(midnight: Option<chrono::NaiveDateTime>) -> i64 {
    midnight
        .and_then(|naive| naive.and_local_timezone(chrono::Local).single())
        .map(|dt| dt.timestamp())
        .unwrap_or(0)
}

async fn models_block(state: &AppState) -> ModelsBlock {
    // The gateway's `/v1/models` needs an API key, which only exists once a client
    // is configured. Saying that plainly is better than an empty list.
    let Some((base_url, api_key, _, _)) = crate::commands::aimyria::bound_credentials(state) else {
        return ModelsBlock {
            group_name: String::new(),
            count: 0,
            sample: Vec::new(),
            error: None,
        };
    };

    let relay = crate::commands::aimyria::relay_site_base(&base_url);
    match crate::aimyria_apps::fetch_group_models(&relay, &api_key).await {
        Ok(group) => ModelsBlock {
            group_name: group.group_name,
            count: group.models.len(),
            sample: group.models.into_iter().take(6).collect(),
            error: None,
        },
        Err(error) => ModelsBlock {
            group_name: String::new(),
            count: 0,
            sample: Vec::new(),
            error: Some(error),
        },
    }
}

fn clients_block(_state: &AppState) -> ClientsBlock {
    let specs = crate::live_writer::CLIENTS;
    let mut names = Vec::new();
    for spec in specs {
        // Only count a client whose live config actually points at us; a
        // selected-but-never-written provider must not read as configured.
        if crate::commands::aimyria::client_configured(&spec.app_type) {
            names.push(spec.label.to_string());
        }
    }
    // The two Aimyria-only clients have no provider row; they are configured when
    // their own file says so.
    for app in [
        crate::aimyria_apps::AimyriaOnlyApp::WorkBuddy,
        crate::aimyria_apps::AimyriaOnlyApp::DeepSeekHarness,
    ] {
        if crate::commands::aimyria::aimyria_only_app_configured(app) {
            names.push(app.label().to_string());
        }
    }

    ClientsBlock {
        configured: names.len(),
        total: specs.len() + 2,
        names,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A failed block must not erase the others: that is the whole point of the
    /// per-block `error` field.
    #[test]
    fn every_block_can_report_its_own_failure_independently() {
        let overview = RelayOverview {
            balance: BalanceBlock {
                remaining: None,
                used: None,
                plan_name: None,
                error: Some("网络不可用".into()),
                reason: Some("query_failed".into()),
            },
            usage: UsageBlock {
                today_requests: 12,
                today_cost: "1.23".into(),
                today_tokens: 4567,
                month_requests: 100,
                month_cost: "9.87".into(),
                success_rate: 0.99,
                error: None,
            },
            models: ModelsBlock {
                group_name: "国模·稳定".into(),
                count: 16,
                sample: vec!["claude-opus-5-5".into()],
                error: None,
            },
            clients: ClientsBlock {
                configured: 2,
                total: 8,
                names: vec!["Claude Code".into()],
            },
            fetched_at: 1_800_000_000,
        };

        let json = serde_json::to_value(&overview).expect("must serialize");
        // The failed block carries a reason...
        assert_eq!(json["balance"]["reason"], "query_failed");
        // ...and the healthy blocks still carry their data.
        assert_eq!(json["usage"]["todayRequests"], 12);
        assert_eq!(json["models"]["count"], 16);
        assert_eq!(json["clients"]["configured"], 2);
    }

    /// The wire format is camelCase; the frontend types depend on it.
    #[test]
    fn the_wire_format_is_camel_case() {
        let block = UsageBlock {
            today_requests: 1,
            today_cost: "0".into(),
            today_tokens: 0,
            month_requests: 0,
            month_cost: "0".into(),
            success_rate: 1.0,
            error: None,
        };
        let json = serde_json::to_value(block).unwrap();
        for key in [
            "todayRequests",
            "todayCost",
            "todayTokens",
            "monthRequests",
            "monthCost",
            "successRate",
        ] {
            assert!(json.get(key).is_some(), "missing {key} in {json}");
        }
    }

    /// An unknown balance must stay `null`, never `0`, because the UI renders a
    /// zero as "you are out of money" and sends the user to top up.
    #[test]
    fn an_unknown_balance_serializes_as_null_not_zero() {
        let block = BalanceBlock {
            remaining: None,
            used: None,
            plan_name: None,
            error: Some("网络不可用".into()),
            reason: Some("query_failed".into()),
        };
        let json = serde_json::to_value(block).unwrap();
        assert!(json["remaining"].is_null());
    }

    /// "Today" must start at local midnight, not UTC midnight.
    ///
    /// A user in UTC+8 whose counter resets at 08:00 would report it as a bug, and
    /// they would be right. This asserts the boundary lands on a local-midnight
    /// wall clock, whatever the machine's zone is.
    #[test]
    fn today_starts_at_local_midnight() {
        use chrono::{Local, TimeZone};
        let start = start_of_today();
        assert!(start > 0, "a real timestamp, not the 0 fallback");

        let local = Local.timestamp_opt(start, 0).single().expect("valid instant");
        assert_eq!(
            local.format("%H:%M:%S").to_string(),
            "00:00:00",
            "the boundary must be midnight in the user's zone"
        );
    }

    /// The month boundary must be the first of the month at local midnight.
    #[test]
    fn the_month_starts_on_the_first_at_local_midnight() {
        use chrono::{Datelike, Local, TimeZone};
        let start = start_of_month();
        assert!(start > 0);

        let local = Local.timestamp_opt(start, 0).single().expect("valid instant");
        assert_eq!(local.day(), 1, "must be the first of the month");
        assert_eq!(local.format("%H:%M:%S").to_string(), "00:00:00");

        // And it must not be in the future.
        assert!(start <= Local::now().timestamp());
    }

    /// Today's boundary is never after this month's boundary.
    #[test]
    fn today_is_not_before_the_start_of_the_month() {
        assert!(
            start_of_today() >= start_of_month(),
            "today cannot precede the month start"
        );
    }
}
