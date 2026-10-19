//! 托盘菜单（v7）
//!
//! 结构：问题区（出问题才有）→ 反馈行（托盘里刚做完要重启才生效的操作）→ 打开 CC Switch →
//! 切换式应用的子菜单（Claude Code、Claude Desktop、Codex、Gemini CLI、Grok Build；在「应用」页
//! 隐藏的不列）→ 轻量模式 → 打开官方网站 / 退出 CC Switch。累加式应用（OpenCode / OpenClaw /
//! Hermes / Pi / MiniMax Code）不进托盘；托盘里不切模式、不启停路由服务。
//!
//! 分三层：`collect_*` 从数据库和设备状态读出快照，`build_menu_model` 把快照变成纯数据的菜单
//! 模型（单测覆盖这一层），`attach_*` 把模型挂到 Tauri 原生菜单上。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use once_cell::sync::Lazy;
use serde::Serialize;
use tauri::image::Image;
use tauri::menu::{
    CheckMenuItem, Menu, MenuBuilder, MenuItem, MenuItemKind, Submenu, SubmenuBuilder,
};
use tauri::{Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::usage_cache::UsageCache;
use crate::store::AppState;

const TEMPLATE_TYPE_OFFICIAL_SUBSCRIPTION: &str = "official_subscription";
/// Copilot 用量结果的单位（`commands::provider` 的 `COPILOT_UNIT_PREMIUM`）：高级请求次数。
const COPILOT_UNIT_PREMIUM: &str = "requests";

pub const TRAY_ID: &str = "aimyria-one";

/// 进托盘的应用，顺序和侧栏一致。累加式应用没有「当前供应商」，不进托盘。
pub const TRAY_APPS: [AppType; 5] = [
    AppType::Claude,
    AppType::ClaudeDesktop,
    AppType::Codex,
    AppType::Gemini,
    AppType::GrokBuild,
];

/// 应用全称（产品名，不翻译）。用全称是为了把 Claude Code 和 Claude Desktop 分开。
fn app_display_name(app: &AppType) -> &'static str {
    match app {
        AppType::Claude => "Claude Code",
        AppType::ClaudeDesktop => "Claude Desktop",
        AppType::Codex => "Codex",
        AppType::Gemini => "Gemini CLI",
        AppType::GrokBuild => "Grok Build",
        AppType::OpenCode => "OpenCode",
        AppType::OpenClaw => "OpenClaw",
        AppType::Hermes => "Hermes",
        AppType::Pi => "Pi",
        AppType::Mcode => "MiniMax Code",
    }
}

/// 问题区最多几行，再多写「还有 N 个问题」。
const MAX_PROBLEM_ROWS: usize = 2;
/// 原生菜单不会自己截断，名字太长会把整个菜单撑宽。
const MAX_NAME_CHARS: usize = 32;
/// 问题区里切换失败的原因最多几个字。
const MAX_REASON_CHARS: usize = 60;
/// 额度剩余不到这个百分比算「快用完」（和前端 `quotaRules.WARN_BELOW_PERCENT` 一致）。
const WARN_BELOW_PERCENT: f64 = 20.0;

/// 每个应用行的子菜单句柄，额度更新时就地改标题而不是整菜单重建（整建会关掉 macOS 上
/// 正开着的菜单）。`create_tray_menu` 每次重建都整表覆盖写入。
static TRAY_SECTION_SUBMENUS: Lazy<Mutex<HashMap<AppType, Submenu<tauri::Wry>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// ─── 文案 ────────────────────────────────────────────────────────────────────

/// 托盘菜单文本（四语）。托盘文案不走前端的 i18n JSON；和界面同义的词照抄前端的译法。
///
/// 模板占位：`{app}` `{name}` `{port}` `{mode}` `{reason}` `{count}` `{value}` `{when}`；
/// 档名用 `{label}`，中文模板用 `{labelSp}`（档名以字母数字结尾时自动补一个空格）。
#[derive(Clone, Copy)]
pub struct TrayTexts {
    pub show_main: &'static str,
    pub open_website: &'static str,
    pub lightweight_mode: &'static str,
    pub quit: &'static str,
    /// 有应用在用路由服务时的「退出」：写明后果（客户端指回直连、路由服务停止，下次打开自动接回）。
    pub quit_stops_routing: &'static str,
    pub projects_label: &'static str,
    pub no_project_label: &'static str,
    pub header_direct: &'static str,
    pub header_route: &'static str,
    pub header_failover: &'static str,
    pub header_stack: &'static str,
    /// 聚合子菜单标题，写出默认那家：`{name}`
    pub header_stack_default: &'static str,
    pub failover_note: &'static str,
    pub mode_route: &'static str,
    pub mode_stack: &'static str,
    pub mode_mapping: &'static str,
    pub needs_routing_suffix: &'static str,
    pub official_blocked_suffix: &'static str,
    pub mapping_suffix: &'static str,
    pub open_app_page: &'static str,
    pub add_provider: &'static str,
    pub almost_out: &'static str,
    pub needs_attention: &'static str,
    pub problem_service_down: &'static str,
    pub problem_attach_failed: &'static str,
    pub problem_switch_failed: &'static str,
    pub problem_more: &'static str,
    pub desktop_unavailable: &'static str,
    /// 出问题时托盘图标的悬停提示：`{problem}` 是问题区第一条
    pub tooltip_problem: &'static str,
    /// 反馈行（灰字）：直连切换 / Claude Desktop 切换，`{app}` `{name}`
    pub feedback_switched: &'static str,
    /// 反馈行：聚合换默认
    pub feedback_stack_default: &'static str,
    /// 反馈行：应用项目
    pub feedback_profile: &'static str,
    pub tier_five_hour: &'static str,
    pub tier_weekly: &'static str,
    pub tier_fable: &'static str,
    pub tier_monthly: &'static str,
    pub tier_thirty_day: &'static str,
    pub tier_credits: &'static str,
    pub tier_gemini_pro: &'static str,
    pub tier_gemini_flash: &'static str,
    pub tier_gemini_flash_lite: &'static str,
    pub tier_premium: &'static str,
    pub tier_left: &'static str,
    pub tier_used: &'static str,
    pub tier_used_up: &'static str,
    pub balance: &'static str,
    pub balance_used_up: &'static str,
    pub plan_expired: &'static str,
    pub used_amount: &'static str,
    pub quota_failed: &'static str,
    pub quota_failed_login_expired: &'static str,
    pub quota_failed_token_refresh_pending: &'static str,
    pub reset_on_date: &'static str,
    pub reset_at_time: &'static str,
    /// chrono 格式串：重置日期
    pub date_format: &'static str,
    /// 按档的额度写已用百分比（设置里「额度显示」选了已用）；不是文案，跟着文案一起传下去
    pub quota_used: bool,
}

/// 将系统区域标识映射为托盘支持的语言码。
///
/// 镜像前端 `i18n/getInitialLanguage` 的判定顺序，确保首次安装
/// （`settings.language` 尚未写入）时托盘语言与界面语言一致：
/// 繁中系统（zh-TW/HK/MO/Hant）→ `zh-TW`，其余 zh → `zh`，
/// 日文 → `ja`，英文 → `en`，未知区域回退到 `zh`（与前端默认一致）。
fn map_locale_to_tray_language(locale: &str) -> &'static str {
    let locale = locale.to_lowercase();
    if locale == "zh" {
        "zh"
    } else if locale.starts_with("zh-tw")
        || locale.starts_with("zh-hk")
        || locale.starts_with("zh-mo")
        || locale.starts_with("zh-hant")
    {
        "zh-TW"
    } else if locale.starts_with("zh") {
        "zh"
    } else if locale.starts_with("ja") {
        "ja"
    } else if locale.starts_with("en") {
        "en"
    } else {
        "zh"
    }
}

/// 读取系统区域并映射为托盘语言码；取不到区域时回退到 `zh`。
fn detect_system_tray_language() -> &'static str {
    sys_locale::get_locale()
        .as_deref()
        .map(map_locale_to_tray_language)
        .unwrap_or("zh")
}

impl TrayTexts {
    pub fn from_language(language: &str) -> Self {
        match language {
            "en" => Self {
                show_main: "Open AimyriA go",
                open_website: "Open official website",
                lightweight_mode: "Lightweight mode",
                quit: "Quit AimyriA go",
                quit_stops_routing: "Quit AimyriA go",
                projects_label: "Projects",
                no_project_label: "No project",
                header_direct: "Direct",
                header_route: "Routing",
                header_failover: "Routing · Failover on",
                header_stack: "Aggregation · Default provider",
                header_stack_default: "Aggregation · Default: {name}",
                failover_note: "Picked from the queue automatically; change it on the app page",
                mode_route: "Routing",
                mode_stack: "Aggregation",
                mode_mapping: "Model mapping",
                needs_routing_suffix: " (needs routing)…",
                official_blocked_suffix: " (official plans don't go through routing)",
                mapping_suffix: " · Model mapping",
                open_app_page: "Open {app} page",
                add_provider: "Add provider…",
                almost_out: "almost out",
                needs_attention: "needs attention",
                problem_service_down: "The routing service isn't running (port {port})",
                problem_attach_failed:
                    "{app}: {mode} couldn't reconnect at startup; back to direct",
                problem_switch_failed: "{app} didn't switch: {reason}",
                problem_more: "{count} more issues — open AimyriA go to see them",
                desktop_unavailable:
                    "The routing service isn't running; {name} is unavailable for now",
                tooltip_problem: "AimyriA go · Needs attention: {problem}",
                feedback_switched: "{app} switched to {name}. Restart {app} to apply",
                feedback_stack_default:
                    "{app}'s default provider is now {name}. Restart {app} to apply",
                feedback_profile: "Applied project \u{201c}{name}\u{201d} to {app}",
                tier_five_hour: "5-hour",
                tier_weekly: "Weekly",
                tier_fable: "Fable",
                tier_monthly: "Monthly",
                tier_thirty_day: "30-day",
                tier_credits: "Credits",
                tier_gemini_pro: "Pro",
                tier_gemini_flash: "Flash",
                tier_gemini_flash_lite: "Flash Lite",
                tier_premium: "Premium",
                tier_left: "{label} {value}% left",
                tier_used: "{label} {value}% used",
                tier_used_up: "{label} used up",
                balance: "Balance {value}",
                balance_used_up: "Balance used up",
                plan_expired: "Plan expired",
                used_amount: "Used {value}",
                quota_failed: "Quota unavailable",
                quota_failed_login_expired: "Quota unavailable: sign-in expired",
                quota_failed_token_refresh_pending: "Quota unavailable: token refresh pending",
                reset_on_date: "{label} quota resets {when}",
                reset_at_time: "{label} quota resets at {when}",
                date_format: "%b %-d",
                quota_used: false,
            },
            "ja" => Self {
                show_main: "AimyriA go を開く",
                open_website: "公式サイトを開く",
                lightweight_mode: "軽量モード",
                quit: "AimyriA go を終了",
                quit_stops_routing: "AimyriA go を終了",
                projects_label: "プロジェクト",
                no_project_label: "プロジェクトを使用しない",
                header_direct: "直接接続",
                header_route: "ルーティング",
                header_failover: "ルーティング · フェイルオーバー有効",
                header_stack: "集約 · デフォルトのプロバイダー",
                header_stack_default: "集約 · デフォルト：{name}",
                failover_note: "キューの順に自動で選ばれます（変更はアプリのページで）",
                mode_route: "ルーティング",
                mode_stack: "集約",
                mode_mapping: "モデルマッピング",
                needs_routing_suffix: "（ルーティングが必要）…",
                official_blocked_suffix: "（公式サブスクリプションはルーティングを通りません）",
                mapping_suffix: " · モデルマッピング",
                open_app_page: "{app} のページを開く",
                add_provider: "プロバイダーを追加…",
                almost_out: "残りわずか",
                needs_attention: "対応が必要",
                problem_service_down: "ルーティングサービスが動いていません（ポート {port}）",
                problem_attach_failed: "{app}：前回の{mode}に再接続できず、直接接続に戻りました",
                problem_switch_failed: "{app} を切り替えられませんでした：{reason}",
                problem_more:
                    "ほかに {count} 件の問題があります。AimyriA go を開いて確認してください",
                desktop_unavailable:
                    "ルーティングサービスが動いていないため、{name} は今使えません",
                tooltip_problem: "AimyriA go · 対応が必要：{problem}",
                feedback_switched:
                    "{app} を {name} に切り替えました。反映するには {app} を再起動してください",
                feedback_stack_default: "{app} のデフォルトのプロバイダーを {name} に変更しました。反映するには {app} を再起動してください",
                feedback_profile: "プロジェクト「{name}」を {app} に適用しました",
                tier_five_hour: "5時間",
                tier_weekly: "週間",
                tier_fable: "Fable",
                tier_monthly: "月間",
                tier_thirty_day: "30日間",
                tier_credits: "クレジット",
                tier_gemini_pro: "Pro",
                tier_gemini_flash: "Flash",
                tier_gemini_flash_lite: "Flash Lite",
                tier_premium: "プレミアム",
                tier_left: "{label} 残り {value}%",
                tier_used: "{label} 使用済み {value}%",
                tier_used_up: "{label} 使い切り",
                balance: "残高 {value}",
                balance_used_up: "残高なし",
                plan_expired: "プラン期限切れ",
                used_amount: "使用 {value}",
                quota_failed: "残量を取得できません",
                quota_failed_login_expired: "残量を取得できません：ログインの期限切れ",
                quota_failed_token_refresh_pending: "残量を取得できません：トークンの更新待ち",
                reset_on_date: "{label}の枠は {when} にリセット",
                reset_at_time: "{label}の枠は {when} にリセット",
                date_format: "%-m月%-d日",
                quota_used: false,
            },
            "zh-TW" => Self {
                show_main: "開啟 AimyriA go",
                open_website: "開啟官方網站",
                lightweight_mode: "輕量模式",
                quit: "退出 AimyriA go",
                quit_stops_routing: "退出 AimyriA go",
                projects_label: "專案",
                no_project_label: "不使用專案",
                header_direct: "直連",
                header_route: "路由",
                header_failover: "路由 · 故障轉移開啟中",
                header_stack: "聚合 · 預設供應商",
                header_stack_default: "聚合 · 預設 {name}",
                failover_note: "依佇列自動選擇，要調整請到應用頁",
                mode_route: "路由",
                mode_stack: "聚合",
                mode_mapping: "模型映射",
                needs_routing_suffix: "（需要路由）…",
                official_blocked_suffix: "（官方訂閱不經過路由）",
                mapping_suffix: " · 模型映射",
                open_app_page: "開啟 {app} 頁面",
                add_provider: "新增供應商…",
                almost_out: "快用完",
                needs_attention: "需要處理",
                problem_service_down: "路由服務沒在執行（連接埠 {port}）",
                problem_attach_failed: "{app}：上次的{mode}沒能接上，已回到直連",
                problem_switch_failed: "{app} 沒切換成功：{reason}",
                problem_more: "還有 {count} 個問題，開啟 AimyriA go 查看",
                desktop_unavailable: "路由服務沒在執行，{name} 暫時無法使用",
                tooltip_problem: "AimyriA go · 需要處理：{problem}",
                feedback_switched: "{app} 已切換到 {name}，重新啟動 {app} 後生效",
                feedback_stack_default: "{app} 的預設供應商改為 {name}，重新啟動 {app} 後生效",
                feedback_profile: "已把專案「{name}」套用到 {app}",
                tier_five_hour: "5 小時",
                tier_weekly: "每週",
                tier_fable: "Fable",
                tier_monthly: "每月",
                tier_thirty_day: "30 天",
                tier_credits: "額度",
                tier_gemini_pro: "Pro",
                tier_gemini_flash: "Flash",
                tier_gemini_flash_lite: "Flash Lite",
                tier_premium: "進階請求",
                tier_left: "{labelSp}剩餘 {value}%",
                tier_used: "{labelSp}已用 {value}%",
                tier_used_up: "{labelSp}已用完",
                balance: "餘額 {value}",
                balance_used_up: "餘額已用完",
                plan_expired: "方案已過期",
                used_amount: "已使用 {value}",
                quota_failed: "額度沒查到",
                quota_failed_login_expired: "額度沒查到：登入已過期",
                quota_failed_token_refresh_pending: "額度沒查到：權杖待重新整理",
                reset_on_date: "{labelSp}額度 {when}重置",
                reset_at_time: "{labelSp}額度 {when} 重置",
                date_format: "%-m 月 %-d 日",
                quota_used: false,
            },
            _ => Self {
                show_main: "打开 AimyriA go",
                open_website: "打开官方网站",
                lightweight_mode: "轻量模式",
                quit: "退出 AimyriA go",
                quit_stops_routing: "退出 AimyriA go",
                projects_label: "项目",
                no_project_label: "不使用项目",
                header_direct: "直连",
                header_route: "路由",
                header_failover: "路由 · 故障转移开启中",
                header_stack: "聚合 · 默认供应商",
                header_stack_default: "聚合 · 默认 {name}",
                failover_note: "按队列自动选择，要调整请到应用页",
                mode_route: "路由",
                mode_stack: "聚合",
                mode_mapping: "模型映射",
                needs_routing_suffix: "（需要路由）…",
                official_blocked_suffix: "（官方订阅不经过路由）",
                mapping_suffix: " · 模型映射",
                open_app_page: "打开 {app} 页面",
                add_provider: "添加供应商…",
                almost_out: "快用完",
                needs_attention: "需要处理",
                problem_service_down: "路由服务没在运行（端口 {port}）",
                problem_attach_failed: "{app}：上次的{mode}没能接上，已回到直连",
                problem_switch_failed: "{app} 没切换成功：{reason}",
                problem_more: "还有 {count} 个问题，打开 AimyriA go 查看",
                desktop_unavailable: "路由服务没在运行，{name} 暂时不可用",
                tooltip_problem: "AimyriA go · 需要处理：{problem}",
                feedback_switched: "{app} 已切换到 {name}，重启 {app} 后生效",
                feedback_stack_default: "{app} 的默认供应商改为 {name}，重启 {app} 后生效",
                feedback_profile: "已把项目「{name}」用到 {app}",
                tier_five_hour: "5 小时",
                tier_weekly: "每周",
                tier_fable: "Fable",
                tier_monthly: "每月",
                tier_thirty_day: "30 天",
                tier_credits: "额度",
                tier_gemini_pro: "Pro",
                tier_gemini_flash: "Flash",
                tier_gemini_flash_lite: "Flash Lite",
                tier_premium: "高级请求",
                tier_left: "{labelSp}剩余 {value}%",
                tier_used: "{labelSp}已用 {value}%",
                tier_used_up: "{labelSp}已用完",
                balance: "余额 {value}",
                balance_used_up: "余额已用完",
                plan_expired: "套餐已过期",
                used_amount: "已使用 {value}",
                quota_failed: "额度没查到",
                quota_failed_login_expired: "额度没查到：登录已过期",
                quota_failed_token_refresh_pending: "额度没查到：令牌待刷新",
                reset_on_date: "{labelSp}额度 {when}重置",
                reset_at_time: "{labelSp}额度 {when} 重置",
                date_format: "%-m 月 %-d 日",
                quota_used: false,
            },
        }
    }

    /// 按设置里的语言取文案；没设过语言（首次安装）时按系统区域，而不是固定简体。
    fn current() -> Self {
        let settings = crate::settings::get_settings();
        let language = match settings.language.as_deref() {
            Some(lang) => lang,
            None => detect_system_tray_language(),
        };
        Self {
            quota_used: settings.quota_shows_used(),
            ..Self::from_language(language)
        }
    }
}

/// 按占位符填模板。
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut text = template.to_string();
    for (key, value) in values {
        text = text.replace(&format!("{{{key}}}"), value);
    }
    text
}

/// 中文里档名以字母数字结尾（「Pro」「每周 Opus」）时，和后面的「剩余」隔一个空格。
fn fill_label(template: &str, label: &str, extra: &[(&str, &str)]) -> String {
    let label_sp = if label.ends_with(|c: char| c.is_ascii_alphanumeric()) {
        format!("{label} ")
    } else {
        label.to_string()
    };
    let mut values = vec![("labelSp", label_sp.as_str()), ("label", label)];
    values.extend_from_slice(extra);
    fill(template, &values)
}

fn truncate_chars(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

// ─── 额度文字（和供应商卡片同一套：按设置写剩余或已用，快用完 / 已用完才多说一句）────────────

/// 托盘里合并的档：周限额的几个别名取最高利用率，Fable 单列；月窗口里 Codex 免费版的 30 天
/// 窗口也算（#3651）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TierGroup {
    FiveHour,
    Weekly,
    Fable,
    Monthly,
    Credits,
    GeminiPro,
    GeminiFlash,
    GeminiFlashLite,
    Premium,
}

#[derive(Debug, Clone, PartialEq)]
struct QuotaLine {
    text: String,
    /// 剩余百分比；余额没有总额时是 `INFINITY`，套餐过期是负数。
    left: f64,
    /// 档名（重置说明用；余额等没有档名）。
    label: Option<&'static str>,
    resets_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuotaFailure {
    /// 没有更具体的原因（脚本结果只有成功 / 失败）。
    Other,
    LoginExpired,
    /// 访问令牌过期、刷新令牌还在：客户端下次运行时自己会换新的。
    TokenRefreshPending,
}

#[derive(Debug, Clone, PartialEq)]
enum QuotaView {
    Lines(Vec<QuotaLine>),
    Failed(QuotaFailure),
}

struct TierEntry<'a> {
    name: &'a str,
    utilization: f64,
    resets_at: Option<String>,
}

fn tier_line(
    texts: &TrayTexts,
    label: &'static str,
    utilization: f64,
    resets_at: Option<String>,
) -> QuotaLine {
    let left = (100.0 - utilization).round().max(0.0);
    let text = if left <= 0.0 {
        fill_label(texts.tier_used_up, label, &[])
    } else if texts.quota_used {
        let value = format!("{}", (100.0 - left) as i64);
        fill_label(texts.tier_used, label, &[("value", value.as_str())])
    } else {
        let value = format!("{}", left as i64);
        fill_label(texts.tier_left, label, &[("value", value.as_str())])
    };
    QuotaLine {
        text,
        left,
        label: Some(label),
        resets_at,
    }
}

/// 已知档位按组合并成额度行（每组取利用率最高的那档）。
/// Group known tiers into quota lines.
///
/// Kept as a single pass so `TierEntry` stays meaningful: with no tiered
/// plans in this build there is nothing to group, so entries render as-is.
fn grouped_tier_lines(texts: &TrayTexts, entries: &[TierEntry<'_>]) -> Vec<QuotaLine> {
    let _ = texts;
    entries
        .iter()
        .map(|entry| QuotaLine {
            text: entry.name.to_string(),
            left: (100.0 - entry.utilization).round().max(0.0),
            label: None,
            resets_at: entry.resets_at.clone(),
        })
        .collect()
}

/// 脚本结果里 `extra` 带的重置时间：Token Plan 是 JSON 的 `resetsAt`，官方订阅是原样的时间串。
fn resets_at_from_extra(extra: Option<&str>) -> Option<String> {
    let extra = extra?.trim();
    if extra.starts_with('{') {
        return serde_json::from_str::<serde_json::Value>(extra)
            .ok()?
            .get("resetsAt")?
            .as_str()
            .map(str::to_string);
    }
    chrono::DateTime::parse_from_rfc3339(extra)
        .ok()
        .map(|_| extra.to_string())
}

fn amount(value: f64, unit: Option<&str>) -> String {
    match unit.map(str::trim).filter(|unit| !unit.is_empty()) {
        Some(unit) => format!("{value:.2} {unit}"),
        None => format!("{value:.2}"),
    }
}

/// 不认识档名的一条脚本结果（余额、Copilot、自定义脚本），照卡片 `UsageFooter.planLine`。
fn plan_line(texts: &TrayTexts, data: &crate::provider::UsageData) -> Option<QuotaLine> {
    if data.is_valid == Some(false) {
        return Some(QuotaLine {
            text: texts.plan_expired.to_string(),
            left: -1.0,
            label: None,
            resets_at: None,
        });
    }
    let unit = data.unit.as_deref();
    let total = data.total.filter(|total| *total != -1.0);
    if unit == Some(COPILOT_UNIT_PREMIUM) {
        if let (Some(remaining), Some(total)) = (data.remaining, total) {
            if total > 0.0 {
                let utilization = (total - remaining) / total * 100.0;
                return Some(tier_line(texts, texts.tier_premium, utilization, None));
            }
        }
    }
    if let Some(remaining) = data.remaining {
        // 余额不提示「快用完」，只有用完才算（和卡片 `quotaRules.balanceLine` 一致）
        let left = if remaining <= 0.0 { 0.0 } else { f64::INFINITY };
        let text = if remaining <= 0.0 {
            texts.balance_used_up.to_string()
        } else {
            fill(texts.balance, &[("value", &amount(remaining, unit))])
        };
        return Some(QuotaLine {
            text,
            left,
            label: None,
            resets_at: None,
        });
    }
    data.used.map(|used| QuotaLine {
        text: fill(texts.used_amount, &[("value", &amount(used, unit))]),
        left: f64::INFINITY,
        label: None,
        resets_at: None,
    })
}

/// Render a usage result as tray lines.
///
/// Our balance endpoint returns a single entry, so each `UsageData` becomes
/// one line. Upstream additionally reassembled named tiers (coding-plan and
/// official-subscription quotas); that grouping is gone with those features.
fn format_script_result(
    texts: &TrayTexts,
    result: &crate::provider::UsageResult,
) -> Option<QuotaView> {
    if !result.success {
        return Some(QuotaView::Failed(QuotaFailure::Other));
    }
    let data = result.data.as_ref()?;

    let lines: Vec<QuotaLine> = data
        .iter()
        .map(|entry| {
            // Prefer the remaining amount; fall back to what was used so a
            // response without `remaining` still shows a number.
            let amount = match (entry.remaining, entry.used) {
                (Some(remaining), _) => remaining,
                (None, Some(used)) => used,
                _ => 0.0,
            };
            let unit = entry.unit.clone().unwrap_or_default();
            let rendered = if unit.is_empty() {
                format!("{amount}")
            } else {
                format!("{amount} {unit}")
            };
            QuotaLine {
                text: fill(texts.balance, &[("value", rendered.as_str())]),
                left: entry.remaining.unwrap_or_default(),
                label: None,
                resets_at: resets_at_from_extra(entry.extra.as_deref()),
            }
        })
        .collect();

    (!lines.is_empty()).then_some(QuotaView::Lines(lines))
}

fn managed_codex_account_id(provider: &Provider) -> Option<String> {
    None
}

fn provider_uses_official_subscription(provider: &Provider) -> bool {
    // Managed Codex uses the account-scoped path in tray_usage_source instead
    // of the CLI's app-wide subscription cache.
    if managed_codex_account_id(provider).is_some() {
        return false;
    }

    provider
        .meta
        .as_ref()
        .and_then(|m| m.usage_script.as_ref())
        .map(|script| {
            script.enabled
                && script.template_type.as_deref() == Some(TEMPLATE_TYPE_OFFICIAL_SUBSCRIPTION)
        })
        .unwrap_or(false)
}

#[derive(Debug, PartialEq, Eq)]
enum TrayUsageSource {
    ManagedCodex(String),
    /// 客户端自己登录的官方订阅：和供应商卡片读写同一份应用级订阅缓存。
    Subscription,
    Script,
}

/// Keep the tray's refresh and display paths on the same credentials and toggle.
fn tray_usage_source(app_type: &AppType, provider: &Provider) -> Option<TrayUsageSource> {
    if *app_type == AppType::Codex {
        if let Some(account_id) = managed_codex_account_id(provider) {
            // Match ProviderCard: managed accounts query by default until the
            // user explicitly disables usage, including older saved providers.
            let enabled = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.usage_script.as_ref())
                .map(|script| script.enabled)
                .unwrap_or(true);
            return enabled.then_some(TrayUsageSource::ManagedCodex(account_id));
        }
    }
    // xAI OAuth 的额度属于绑定的 SuperGrok 账号，不是所在应用的客户端登录，仍走脚本路径。
    if provider_uses_official_subscription(provider) && !provider.is_xai_oauth() {
        return Some(TrayUsageSource::Subscription);
    }
    (provider.has_usage_script_enabled()
        && (provider.category.as_deref() != Some("official")
            || provider_uses_official_subscription(provider)))
    .then_some(TrayUsageSource::Script)
}

/// 在用的那家的额度。🔴 #7267：托管 Codex 账号卡按账号取，缓存缺失时**不能**回落到应用级
/// 订阅缓存或供应商级脚本缓存。
/// Quota for the provider currently in use.
///
/// Upstream also consulted managed-OAuth and app-subscription caches here.
/// Those backends were removed in M2, so the script cache (fed by our own
/// /v1/usage balance query) is the only source left.
fn usage_view(
    usage_cache: &UsageCache,
    texts: &TrayTexts,
    app_type: &AppType,
    provider: &Provider,
    provider_id: &str,
) -> Option<QuotaView> {
    // A disabled or deleted script must not keep showing a stale snapshot.
    if tray_usage_source(app_type, provider).is_none() {
        usage_cache.invalidate_script(app_type, provider_id);
        return None;
    }
    // Another provider's balance must never be shown under this one.
    usage_cache
        .with_script(app_type, provider_id, |result| {
            format_script_result(texts, result)
        })
        .flatten()
}

// ─── 「需要路由」判定（镜像前端 `providerNeedsRouting`）──────────────────────────────

const MANAGED_OAUTH_PROVIDER_TYPES: &[&str] = &["github_copilot", "codex_oauth", "xai_oauth"];

/// Codex / Grok Build 配置里当前供应商的 `wire_api`；TOML 读不懂时退回唯一的一条赋值。
fn codex_wire_api(config_text: &str) -> Option<String> {
    if let Ok(doc) = config_text.parse::<toml::Value>() {
        if let Some(active) = doc.get("model_provider").and_then(|v| v.as_str()) {
            if let Some(wire_api) = doc
                .get("model_providers")
                .and_then(|providers| providers.get(active))
                .and_then(|provider| provider.get("wire_api"))
                .and_then(|v| v.as_str())
            {
                return Some(wire_api.to_string());
            }
        }
        return doc
            .get("wire_api")
            .and_then(|v| v.as_str())
            .map(str::to_string);
    }
    let mut found = config_text.lines().filter_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "wire_api").then(|| {
            value
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string()
        })
    });
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

fn is_chat_or_anthropic_wire_api(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "chat"
            | "chat_completions"
            | "chat-completions"
            | "openai_chat"
            | "openai-chat"
            | "openai_chat_completions"
            | "anthropic"
            | "anthropic_messages"
            | "anthropic-messages"
            | "messages"
            | "claude"
    )
}

/// 这家在这个应用下必须经过路由服务才能用（直连写进去也用不了）。和前端
/// `providerNeedsRouting`（`src/utils/providerCapabilities.ts`）是同一条规则，改一边要改另一边。
pub(crate) fn provider_needs_routing(app: &AppType, provider: &Provider) -> bool {
    let meta = provider.meta.as_ref();
    let managed_oauth = meta
        .and_then(|meta| meta.provider_type.as_deref())
        .is_some_and(|kind| MANAGED_OAUTH_PROVIDER_TYPES.contains(&kind));
    let full_url = meta.and_then(|meta| meta.is_full_url) == Some(true);
    let api_format = meta
        .and_then(|meta| meta.api_format.as_deref())
        .filter(|fmt| !fmt.is_empty());
    match app {
        AppType::ClaudeDesktop => {
            managed_oauth
                || meta
                    .and_then(|meta| meta.claude_desktop_mode.as_ref())
                    .is_some_and(|mode| *mode == crate::provider::ClaudeDesktopMode::Proxy)
        }
        AppType::Claude => {
            managed_oauth || full_url || api_format.is_some_and(|f| f != "anthropic")
        }
        AppType::Codex | AppType::GrokBuild => {
            managed_oauth
                || full_url
                || matches!(api_format, Some("openai_chat" | "anthropic"))
                || provider
                    .settings_config
                    .get("config")
                    .and_then(|config| config.as_str())
                    .and_then(codex_wire_api)
                    .is_some_and(|wire_api| is_chat_or_anthropic_wire_api(&wire_api))
        }
        _ => false,
    }
}

// ─── 快照 ────────────────────────────────────────────────────────────────────

/// 应用子菜单的样子。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayMode {
    Direct,
    Route,
    /// 路由 + 自动故障转移：子菜单只读（#6022）。
    Failover,
    Stack,
    /// Claude Desktop 没有模式 tab。
    Desktop,
}

#[derive(Debug, Clone, PartialEq)]
struct ProviderEntry {
    id: String,
    name: String,
    /// 同名时补在名字后面的区分词（备注或网址）。
    hint: Option<String>,
    needs_routing: bool,
    official: bool,
    blocked_from_routing: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct ProfileSection {
    scope: &'static str,
    /// (id, 名字)
    items: Vec<(String, String)>,
    current: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct AppSnapshot {
    app: AppType,
    mode: TrayMode,
    /// 按供应商页的顺序（sort_index → created_at → name）。
    providers: Vec<ProviderEntry>,
    /// 在用的那家（`provider_for(InUse)`）。
    current_id: Option<String>,
    /// 故障转移队列（按优先级）。
    queue: Vec<String>,
    /// 聚合名单。
    stack_members: Vec<String>,
    quota: Option<QuotaView>,
    /// 路由服务该在跑却没在跑。
    service_down: bool,
    needs_attention: bool,
    profiles: Option<ProfileSection>,
}

impl AppSnapshot {
    fn current(&self) -> Option<&ProviderEntry> {
        let id = self.current_id.as_deref()?;
        self.providers.iter().find(|p| p.id == id)
    }

    /// 这个应用现在靠路由服务：路由 / 聚合中，或 Desktop 在用模型映射卡。
    fn uses_service(&self) -> bool {
        match self.mode {
            TrayMode::Route | TrayMode::Failover | TrayMode::Stack => true,
            TrayMode::Desktop => self.current().is_some_and(|p| p.needs_routing),
            TrayMode::Direct => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum TrayProblem {
    /// 路由服务没在跑，但有应用在用它。
    ServiceDown { port: u16 },
    /// 启动时没能接上路由 / 聚合，已退回直连。
    AttachFailed { app: AppType, stack: bool },
    /// 托盘里的切换没成功。
    SwitchFailed { app: AppType, reason: String },
}

/// 对供应商列表排序：sort_index → created_at → name
fn sort_providers(providers: &indexmap::IndexMap<String, Provider>) -> Vec<(&String, &Provider)> {
    let mut sorted: Vec<_> = providers.iter().collect();
    sorted.sort_by(|(_, a), (_, b)| {
        match (a.sort_index, b.sort_index) {
            (Some(idx_a), Some(idx_b)) => return idx_a.cmp(&idx_b),
            (Some(_), None) => return std::cmp::Ordering::Less,
            (None, Some(_)) => return std::cmp::Ordering::Greater,
            _ => {}
        }

        match (a.created_at, b.created_at) {
            (Some(time_a), Some(time_b)) => return time_a.cmp(&time_b),
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            _ => {}
        }

        a.name.cmp(&b.name)
    });
    sorted
}

fn provider_hint(provider: &Provider) -> Option<String> {
    if let Some(notes) = provider
        .notes
        .as_deref()
        .and_then(|notes| notes.lines().next())
        .map(str::trim)
        .filter(|notes| !notes.is_empty())
    {
        return Some(truncate_chars(notes, 24));
    }
    let url = provider.website_url.as_deref()?.trim();
    let host = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()?
        .trim();
    (!host.is_empty()).then(|| host.to_string())
}

fn provider_entry(app: &AppType, provider: &Provider) -> ProviderEntry {
    ProviderEntry {
        id: provider.id.clone(),
        name: provider.name.clone(),
        hint: provider_hint(provider),
        needs_routing: provider_needs_routing(app, provider),
        official: provider.category.as_deref() == Some("official"),
        blocked_from_routing: false,
    }
}

/// Switch failures the user has not acted on yet.
///
/// `collect_problems` derives its problems from state that can be re-read at
/// any moment, so it has nowhere to put "the switch you just did failed". That
/// is an event, not a state, so it is remembered here until the user retries or
/// clears it.
static SWITCH_FAILURES: Lazy<Mutex<HashMap<AppType, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Whether the tray icon should carry the amber problem dot.
static PROBLEM_INDICATOR: Lazy<Mutex<Option<bool>>> = Lazy::new(|| Mutex::new(None));

/// Forget an app's recorded failure. Returns whether there was one.
fn clear_app_problems(app: &AppType) -> bool {
    SWITCH_FAILURES.lock().unwrap().remove(app).is_some()
}

/// 主界面打开了某个应用的页面：这个应用的问题行算处理过了（规格 5.5「打开过对应页面」）。
#[tauri::command]
pub fn tray_app_page_seen(app: tauri::AppHandle, app_type: String) {
    let Ok(app_type) = app_type.parse::<AppType>() else {
        return;
    };
    if clear_app_problems(&app_type) {
        schedule_tray_status_check(&app);
    }
}
fn record_switch_failure(app: &AppType, reason: String) {
    SWITCH_FAILURES.lock().unwrap().insert(app.clone(), reason);
}

/// Recorded switch failures, appended to the derived problems.
fn recorded_switch_problems() -> Vec<TrayProblem> {
    SWITCH_FAILURES
        .lock()
        .unwrap()
        .iter()
        .map(|(app, reason)| TrayProblem::SwitchFailed {
            app: app.clone(),
            reason: reason.clone(),
        })
        .collect()
}

fn service_running(_app_state: &AppState) -> Option<bool> {
    // We run no local routing service, so there is nothing that can be down.
    // Reported as `Some(true)` so callers that treat `None` as "unknown" keep
    // treating the tool as healthy.
    Some(true)
}

/// 问题列表。`modes` 是可见应用现在的模式（重新进入路由的应用不再报「退回直连」）。
fn collect_problems(
    service_down_port: Option<u16>,
    visible: &[(AppType, bool)],
) -> Vec<TrayProblem> {
    let mut problems = Vec::new();
    if let Some(port) = service_down_port {
        problems.push(TrayProblem::ServiceDown { port });
    }
    {
        // 已经重新进入路由 / 聚合的应用不再报。
    }
    {}
    problems
}

/// 受影响的应用行尾写「需要处理」（这时不写额度，保持短）。
fn mark_attention(snapshots: &mut [AppSnapshot], problems: &[TrayProblem]) {
    for snapshot in snapshots.iter_mut() {
        snapshot.needs_attention = snapshot.service_down
            || problems.iter().any(|problem| {
                matches!(problem, TrayProblem::AttachFailed { app, .. } if *app == snapshot.app)
            });
    }
}

// ─── 反馈行 ───────────────────────────────────────────────────────────────────

/// 反馈行显示多久（Linux 没有点击事件，只按时间）。
const FEEDBACK_TTL: std::time::Duration = std::time::Duration::from_secs(2 * 60);

/// 托盘里做完一个要重启才生效（或应用了项目）的操作后，下次打开菜单时最上面那行灰字。
/// 存结构不存文字：语言改了照样按新语言写。
#[derive(Debug, Clone, PartialEq)]
enum TrayFeedback {
    /// Codex / Gemini CLI / Grok Build 直连切换，或 Claude Desktop 切换。
    Switched { app: AppType, name: String },
    /// 聚合换默认：发布给客户端的模型列表跟着变。
    StackDefault { app: AppType, name: String },
    /// 应用项目：同时改了供应商、MCP、Skills、提示词。
    ProfileApplied { app: AppType, name: String },
}

struct FeedbackRecord {
    feedback: TrayFeedback,
    at: std::time::Instant,
    /// 已经在打开的菜单里显示过一次（点过托盘图标）。
    seen: bool,
}

static FEEDBACK: Lazy<Mutex<Option<FeedbackRecord>>> = Lazy::new(|| Mutex::new(None));

/// 切换成功后要不要说一句：客户端只在启动时读配置的才说；Claude Code 直连、路由 / 故障转移
/// 换一家立即生效，不说。
fn switch_feedback(app: &AppType, mode: TrayMode, name: String) -> Option<TrayFeedback> {
    match mode {
        TrayMode::Desktop => Some(TrayFeedback::Switched {
            app: app.clone(),
            name,
        }),
        TrayMode::Direct
            if matches!(app, AppType::Codex | AppType::Gemini | AppType::GrokBuild) =>
        {
            Some(TrayFeedback::Switched {
                app: app.clone(),
                name,
            })
        }
        TrayMode::Stack => Some(TrayFeedback::StackDefault {
            app: app.clone(),
            name,
        }),
        _ => None,
    }
}

/// 记下最近一次托盘操作的结果（覆盖上一条）。调用方随后会重建菜单。
fn record_feedback(app: &tauri::AppHandle, feedback: TrayFeedback) {
    // Linux 没有悬停 / 点击事件，到点了自己重建一次把它拿掉。其它平台在悬停图标时就会重建，
    // 不在这里定时重建，免得把正开着的菜单关掉。
    #[cfg(target_os = "linux")]
    {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(FEEDBACK_TTL + std::time::Duration::from_secs(1));
            refresh_tray_if_status_changed(&app);
        });
    }
    #[cfg(not(target_os = "linux"))]
    let _ = app;
}

/// 现在该显示的反馈：没过期、也还没在打开的菜单里显示过。
fn current_feedback() -> Option<TrayFeedback> {
    FEEDBACK
        .lock()
        .unwrap()
        .as_ref()
        .filter(|record| record.at.elapsed() < FEEDBACK_TTL)
        .map(|record| record.feedback.clone())
}

fn feedback_text(texts: &TrayTexts, feedback: &TrayFeedback) -> String {
    let (template, app, name) = match feedback {
        TrayFeedback::Switched { app, name } => (texts.feedback_switched, app, name),
        TrayFeedback::StackDefault { app, name } => (texts.feedback_stack_default, app, name),
        TrayFeedback::ProfileApplied { app, name } => (texts.feedback_profile, app, name),
    };
    fill(
        template,
        &[
            ("app", app_display_name(app)),
            ("name", &truncate_chars(name, MAX_NAME_CHARS)),
        ],
    )
}

/// 菜单上会自己过时的那部分：问题区、反馈行、「退出」写不写后果。
#[derive(Debug, Clone, Default, PartialEq)]
struct MenuStatus {
    problems: Vec<TrayProblem>,
    feedback: Option<TrayFeedback>,
    routing_in_use: bool,
}

// ─── 菜单模型（纯数据）────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum TrayEntry {
    Item {
        id: String,
        text: String,
        enabled: bool,
    },
    Check {
        id: String,
        text: String,
        enabled: bool,
        checked: bool,
    },
    Submenu {
        id: String,
        text: String,
        /// 应用行：额度更新时按它就地改标题。
        app: Option<AppType>,
        children: Vec<TrayEntry>,
    },
    Separator,
}

impl TrayEntry {
    fn item(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Item {
            id: id.into(),
            text: text.into(),
            enabled: true,
        }
    }

    fn label(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Item {
            id: id.into(),
            text: text.into(),
            enabled: false,
        }
    }

    fn check(id: impl Into<String>, text: impl Into<String>, enabled: bool, checked: bool) -> Self {
        Self::Check {
            id: id.into(),
            text: text.into(),
            enabled,
            checked,
        }
    }
}

fn provider_id_for(app: &AppType, provider_id: &str) -> String {
    format!("prov:{}:{provider_id}", app.as_str())
}

fn problem_text(texts: &TrayTexts, problem: &TrayProblem) -> String {
    match problem {
        TrayProblem::ServiceDown { port } => {
            fill(texts.problem_service_down, &[("port", &port.to_string())])
        }
        TrayProblem::AttachFailed { app, stack } => fill(
            texts.problem_attach_failed,
            &[
                ("app", app_display_name(app)),
                (
                    "mode",
                    if *stack {
                        texts.mode_stack
                    } else {
                        texts.mode_route
                    },
                ),
            ],
        ),
        TrayProblem::SwitchFailed { app, reason } => fill(
            texts.problem_switch_failed,
            &[
                ("app", app_display_name(app)),
                ("reason", &truncate_chars(reason, MAX_REASON_CHARS)),
            ],
        ),
    }
}

fn problem_id(problem: &TrayProblem) -> String {
    match problem {
        TrayProblem::ServiceDown { .. } => "nav:settings:routing".to_string(),
        TrayProblem::AttachFailed { app, .. } => format!("nav:app:{}:attach", app.as_str()),
        TrayProblem::SwitchFailed { app, .. } => format!("nav:app:{}:failed", app.as_str()),
    }
}

/// 应用行标题：`<应用全称> · [模式词 · ]<在用供应商>[ · <额度>][ · 快用完][ · 需要处理]`。
/// One-line quota summary for the app row, plus whether it is nearly out.
///
/// `almost_out` is a separate flag rather than a substring test so the
/// caller can style "余额偏低" differently from the amount itself.
fn quota_title(texts: &TrayTexts, snapshot: &AppSnapshot) -> Option<(String, bool)> {
    let lines = match snapshot.quota.as_ref()? {
        QuotaView::Lines(lines) if !lines.is_empty() => lines,
        _ => return None,
    };
    let title = lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let almost_out = lines
        .iter()
        .any(|line| line.left.is_finite() && line.left < WARN_BELOW_PERCENT);
    let _ = texts;
    Some((title, almost_out))
}

fn app_row_title(texts: &TrayTexts, snapshot: &AppSnapshot) -> String {
    let mut parts: Vec<String> = vec![app_display_name(&snapshot.app).to_string()];
    let current = snapshot.current();
    let mode_word = match snapshot.mode {
        TrayMode::Route | TrayMode::Failover => Some(texts.mode_route),
        TrayMode::Stack => Some(texts.mode_stack),
        TrayMode::Desktop if current.is_some_and(|p| p.needs_routing) => Some(texts.mode_mapping),
        _ => None,
    };
    if let Some(word) = mode_word {
        parts.push(word.to_string());
    }
    if let Some(provider) = current {
        parts.push(truncate_chars(&provider.name, MAX_NAME_CHARS));
    }
    if snapshot.needs_attention {
        parts.push(texts.needs_attention.to_string());
    } else if let Some((quota, almost_out)) = snapshot
        .quota
        .as_ref()
        .and_then(|q| quota_title(texts, snapshot))
    {
        if !quota.is_empty() {
            parts.push(quota);
        }
        if almost_out {
            parts.push(texts.almost_out.to_string());
        }
    }
    parts.join(" · ")
}

/// 列出来的几家的显示名：截断，同名的补备注 / 网址，再不行补序号。
fn display_names(providers: &[&ProviderEntry]) -> Vec<String> {
    providers
        .iter()
        .enumerate()
        .map(|(index, provider)| {
            let name = truncate_chars(&provider.name, MAX_NAME_CHARS);
            let same: Vec<usize> = providers
                .iter()
                .enumerate()
                .filter(|(_, other)| other.name.trim() == provider.name.trim())
                .map(|(i, _)| i)
                .collect();
            if same.len() < 2 {
                return name;
            }
            match provider.hint.as_deref() {
                Some(hint) => format!("{name} · {hint}"),
                None => {
                    let ordinal = same.iter().position(|i| *i == index).unwrap_or(0) + 1;
                    format!("{name} #{ordinal}")
                }
            }
        })
        .collect()
}

fn provider_rows(texts: &TrayTexts, snapshot: &AppSnapshot) -> Vec<TrayEntry> {
    let app = &snapshot.app;
    let current = snapshot.current_id.as_deref();
    let is_current = |p: &ProviderEntry| current == Some(p.id.as_str());
    match snapshot.mode {
        TrayMode::Direct => {
            let listed: Vec<&ProviderEntry> = snapshot.providers.iter().collect();
            let names = display_names(&listed);
            listed
                .iter()
                .zip(names)
                .map(|(p, name)| {
                    if p.needs_routing {
                        // 直连下不能直接切：打开应用页，弹和主界面同一个「需要路由」对话框。
                        TrayEntry::item(
                            format!("nav:needs:{}:{}", app.as_str(), p.id),
                            format!("{name}{}", texts.needs_routing_suffix),
                        )
                    } else {
                        TrayEntry::check(provider_id_for(app, &p.id), name, true, is_current(p))
                    }
                })
                .collect()
        }
        TrayMode::Route => {
            let listed: Vec<&ProviderEntry> = snapshot.providers.iter().collect();
            let names = display_names(&listed);
            listed
                .iter()
                .zip(names)
                .map(|(p, name)| {
                    if p.blocked_from_routing {
                        TrayEntry::label(
                            format!("info:{}:blocked:{}", app.as_str(), p.id),
                            format!("{name}{}", texts.official_blocked_suffix),
                        )
                    } else {
                        TrayEntry::check(provider_id_for(app, &p.id), name, true, is_current(p))
                    }
                })
                .collect()
        }
        TrayMode::Failover => {
            // 只读：只列队列，勾 = 现在路由到的那家；托盘不再悄悄关掉故障转移（#6022）。
            let listed: Vec<&ProviderEntry> = snapshot
                .queue
                .iter()
                .filter_map(|id| snapshot.providers.iter().find(|p| p.id == *id))
                .collect();
            let names = display_names(&listed);
            let mut rows = vec![TrayEntry::label(
                format!("info:{}:failover", app.as_str()),
                texts.failover_note,
            )];
            rows.extend(
                listed
                    .iter()
                    .zip(names)
                    .enumerate()
                    .map(|(index, (p, name))| {
                        TrayEntry::check(
                            provider_id_for(app, &p.id),
                            format!("{}. {name}", index + 1),
                            false,
                            is_current(p),
                        )
                    }),
            );
            rows
        }
        TrayMode::Stack => {
            // 只列名单里的成员和能做默认的官方卡（Codex 官方卡不能加入聚合，但能做默认）；点一家 = 设为默认。
            let listed: Vec<&ProviderEntry> = snapshot
                .providers
                .iter()
                .filter(|p| {
                    is_current(p)
                        || (!p.blocked_from_routing
                            && (snapshot.stack_members.contains(&p.id) || p.official))
                })
                .collect();
            let names = display_names(&listed);
            listed
                .iter()
                .zip(names)
                .map(|(p, name)| {
                    TrayEntry::check(provider_id_for(app, &p.id), name, true, is_current(p))
                })
                .collect()
        }
        TrayMode::Desktop => {
            let listed: Vec<&ProviderEntry> = snapshot.providers.iter().collect();
            let names = display_names(&listed);
            listed
                .iter()
                .zip(names)
                .map(|(p, name)| {
                    let text = if p.needs_routing {
                        format!("{name}{}", texts.mapping_suffix)
                    } else {
                        name
                    };
                    TrayEntry::check(provider_id_for(app, &p.id), text, true, is_current(p))
                })
                .collect()
        }
    }
}

/// Longer quota explanation shown as its own clickable row.
fn quota_note(
    texts: &TrayTexts,
    quota: &QuotaView,
    _now: chrono::DateTime<chrono::Local>,
) -> Option<String> {
    let lines = match quota {
        QuotaView::Lines(lines) => lines,
        QuotaView::Failed(_) => return None,
    };
    if lines.is_empty() {
        return None;
    }
    let mut note = lines
        .iter()
        .filter_map(|line| match &line.resets_at {
            Some(at) => Some(format!("{} · {}", line.text, at)),
            None => Some(line.text.clone()),
        })
        .collect::<Vec<_>>()
        .join("\n");
    note = truncate_chars(&note, 200);
    let _ = texts;
    Some(note)
}

fn app_children(
    texts: &TrayTexts,
    snapshot: &AppSnapshot,
    now: chrono::DateTime<chrono::Local>,
) -> Vec<TrayEntry> {
    let app = snapshot.app.as_str();
    let mut children = Vec::new();

    let header = match snapshot.mode {
        TrayMode::Direct => Some(texts.header_direct.to_string()),
        TrayMode::Route => Some(texts.header_route.to_string()),
        TrayMode::Failover => Some(texts.header_failover.to_string()),
        // 聚合：标题写出默认那家（勾着的就是它）；还没有默认时退回泛称。
        TrayMode::Stack => Some(match snapshot.current() {
            Some(current) => fill(
                texts.header_stack_default,
                &[("name", &truncate_chars(&current.name, MAX_NAME_CHARS))],
            ),
            None => texts.header_stack.to_string(),
        }),
        TrayMode::Desktop => None,
    };
    if let Some(header) = header {
        children.push(TrayEntry::label(format!("info:{app}:header"), header));
    }
    if snapshot.mode == TrayMode::Desktop && snapshot.service_down {
        if let Some(current) = snapshot.current() {
            children.push(TrayEntry::item(
                format!("nav:app:{app}:service"),
                fill(
                    texts.desktop_unavailable,
                    &[("name", &truncate_chars(&current.name, MAX_NAME_CHARS))],
                ),
            ));
            children.push(TrayEntry::Separator);
        }
    }

    children.extend(provider_rows(texts, snapshot));

    // 额度说明行：可点（打开应用页），文字才是正常对比度。
    if let Some(note) = snapshot
        .quota
        .as_ref()
        .and_then(|quota| quota_note(texts, quota, now))
    {
        children.push(TrayEntry::item(format!("nav:app:{app}:quota"), note));
    }

    if let Some(profiles) = &snapshot.profiles {
        children.push(TrayEntry::Separator);
        children.push(TrayEntry::label(
            format!("info:{app}:projects"),
            texts.projects_label,
        ));
        children.push(TrayEntry::check(
            format!("profile_none_{}", profiles.scope),
            texts.no_project_label,
            true,
            profiles.current.is_none(),
        ));
        for (id, name) in &profiles.items {
            children.push(TrayEntry::check(
                format!("profile_{}_{id}", profiles.scope),
                truncate_chars(name, MAX_NAME_CHARS),
                true,
                profiles.current.as_deref() == Some(id.as_str()),
            ));
        }
    }

    children.push(TrayEntry::Separator);
    children.push(TrayEntry::item(
        format!("nav:app:{app}"),
        fill(
            texts.open_app_page,
            &[("app", app_display_name(&snapshot.app))],
        ),
    ));
    children
}

fn app_entry(
    texts: &TrayTexts,
    snapshot: &AppSnapshot,
    now: chrono::DateTime<chrono::Local>,
) -> TrayEntry {
    let app = snapshot.app.as_str();
    if snapshot.providers.is_empty() {
        return TrayEntry::item(
            format!("nav:add:{app}"),
            format!(
                "{} · {}",
                app_display_name(&snapshot.app),
                texts.add_provider
            ),
        );
    }
    TrayEntry::Submenu {
        id: format!("submenu_{app}"),
        text: app_row_title(texts, snapshot),
        app: Some(snapshot.app.clone()),
        children: app_children(texts, snapshot, now),
    }
}

fn build_menu_model(
    texts: &TrayTexts,
    status: &MenuStatus,
    apps: &[AppSnapshot],
    lightweight: bool,
    now: chrono::DateTime<chrono::Local>,
) -> Vec<TrayEntry> {
    let mut menu = Vec::new();
    let problems = &status.problems;

    if !problems.is_empty() {
        for problem in problems.iter().take(MAX_PROBLEM_ROWS) {
            menu.push(TrayEntry::item(
                problem_id(problem),
                problem_text(texts, problem),
            ));
        }
        if problems.len() > MAX_PROBLEM_ROWS {
            let count = (problems.len() - MAX_PROBLEM_ROWS).to_string();
            menu.push(TrayEntry::item(
                "nav:main",
                fill(texts.problem_more, &[("count", &count)]),
            ));
        }
    }
    // 反馈行：灰字、不可点，只说一句结果（文案同主界面 toast）。
    if let Some(feedback) = &status.feedback {
        menu.push(TrayEntry::label(
            "info:feedback",
            feedback_text(texts, feedback),
        ));
    }
    if !problems.is_empty() || status.feedback.is_some() {
        menu.push(TrayEntry::Separator);
    }

    // 窗口找不回来时托盘是唯一入口，「打开 CC Switch」一直放在最上面。
    menu.push(TrayEntry::item("show_main", texts.show_main));
    menu.push(TrayEntry::Separator);

    if !apps.is_empty() {
        menu.extend(apps.iter().map(|snapshot| app_entry(texts, snapshot, now)));
        menu.push(TrayEntry::Separator);
    }

    menu.push(TrayEntry::check(
        "lightweight_mode",
        texts.lightweight_mode,
        true,
        lightweight,
    ));
    menu.push(TrayEntry::Separator);
    menu.push(TrayEntry::item("open_website", texts.open_website));
    // 不换成系统自带的 quit：它不经过 app.exit(0) 的退出流程，客户端指回直连、停服务只能在
    // RunEvent::Exit 里限时补做（见 lib.rs 的 cleanup_before_system_exit）。
    // 有应用在用路由服务时把这个后果写在「退出」上（后果不进说明，写在按钮文字里）。
    menu.push(TrayEntry::item(
        "quit",
        if status.routing_in_use {
            texts.quit_stops_routing
        } else {
            texts.quit
        },
    ));
    menu
}

// ─── 读状态 → 模型 ─────────────────────────────────────────────────────────────

struct TrayModel {
    entries: Vec<TrayEntry>,
    status: MenuStatus,
}

fn collect_snapshots(
    app_state: &AppState,
    texts: &TrayTexts,
) -> Result<(Vec<AppSnapshot>, Vec<TrayProblem>), AppError> {
    let settings = crate::settings::get_settings();
    let visible_apps = settings.visible_apps.clone().unwrap_or_default();

    let mut snapshots: Vec<AppSnapshot> = Vec::new();
    for app in TRAY_APPS.iter().filter(|app| visible_apps.is_visible(app)) {
        let all = app_state.db.get_all_providers(app.as_str())?;
        let providers: Vec<ProviderEntry> = sort_providers(&all)
            .into_iter()
            .map(|(_, provider)| provider_entry(app, provider))
            .collect();
        let current_id = crate::settings::get_current_provider(app);
        let current_provider = current_id
            .as_deref()
            .and_then(|id| all.get(id))
            .cloned()
            .unwrap_or_else(|| {
                Provider::with_id(String::new(), String::new(), serde_json::Value::Null, None)
            });

        // Computed before the struct literal so `current_id` can still be
        // borrowed here and moved in below.
        let quota = usage_view(
            &app_state.usage_cache,
            texts,
            app,
            &current_provider,
            current_id.as_deref().unwrap_or_default(),
        );

        snapshots.push(AppSnapshot {
            app: app.clone(),
            mode: TrayMode::Direct,
            providers,
            current_id,
            // No failover queue and no aggregate stack: both belonged to the
            // local routing feature.
            queue: Vec::new(),
            stack_members: Vec::new(),
            quota,
            service_down: false,
            needs_attention: false,
            profiles: None,
        });
    }

    let visible: Vec<(AppType, bool)> = snapshots
        .iter()
        .map(|snapshot| (snapshot.app.clone(), snapshot.mode == TrayMode::Direct))
        .collect();
    // We run no local routing service, so there is no port to probe.
    let mut problems = collect_problems(None, &visible);
    problems.extend(recorded_switch_problems());
    mark_attention(&mut snapshots, &problems);
    Ok((snapshots, problems))
}

fn collect_status(
    app_state: &AppState,
    texts: &TrayTexts,
) -> Result<(Vec<AppSnapshot>, MenuStatus), AppError> {
    let (snapshots, problems) = collect_snapshots(app_state, texts)?;
    let status = MenuStatus {
        problems,
        feedback: current_feedback(),
        routing_in_use: routing_in_use(app_state),
    };
    Ok((snapshots, status))
}

fn collect_model(app_state: &AppState, texts: &TrayTexts) -> Result<TrayModel, AppError> {
    let (snapshots, status) = collect_status(app_state, texts)?;
    let entries = build_menu_model(
        texts,
        &status,
        &snapshots,
        crate::lightweight::is_lightweight_mode(),
        chrono::Local::now(),
    );
    Ok(TrayModel { entries, status })
}

/// 退出会让它们断开的情况：路由服务在跑，且有应用在路由 / 聚合模式，或 Claude Desktop 在用
/// 模型映射卡（和 `controller::stop_server_if_unused` 的「在用」同一口径，隐藏的应用也算）。
fn routing_in_use(_app_state: &AppState) -> bool {
    // No local routing service exists in this build.
    false
}

// ─── 模型 → Tauri 菜单 ─────────────────────────────────────────────────────────

fn menu_error(e: impl std::fmt::Display) -> AppError {
    AppError::Message(format!("创建托盘菜单失败: {e}"))
}

fn attach_entry(
    app: &tauri::AppHandle,
    entry: &TrayEntry,
    handles: &mut HashMap<AppType, Submenu<tauri::Wry>>,
) -> Result<Option<MenuItemKind<tauri::Wry>>, AppError> {
    Ok(Some(match entry {
        TrayEntry::Separator => return Ok(None),
        TrayEntry::Item { id, text, enabled } => MenuItemKind::MenuItem(
            MenuItem::with_id(app, id.as_str(), text, *enabled, None::<&str>)
                .map_err(menu_error)?,
        ),
        TrayEntry::Check {
            id,
            text,
            enabled,
            checked,
        } => MenuItemKind::Check(
            CheckMenuItem::with_id(app, id.as_str(), text, *enabled, *checked, None::<&str>)
                .map_err(menu_error)?,
        ),
        TrayEntry::Submenu {
            id,
            text,
            app: app_type,
            children,
        } => {
            let mut builder = SubmenuBuilder::with_id(app, id.as_str(), text);
            for child in children {
                builder = match attach_entry(app, child, handles)? {
                    Some(item) => builder.item(&item),
                    None => builder.separator(),
                };
            }
            let submenu = builder.build().map_err(menu_error)?;
            if let Some(app_type) = app_type {
                handles.insert(app_type.clone(), submenu.clone());
            }
            MenuItemKind::Submenu(submenu)
        }
    }))
}

/// 创建动态托盘菜单
pub fn create_tray_menu(
    app: &tauri::AppHandle,
    app_state: &AppState,
) -> Result<Menu<tauri::Wry>, AppError> {
    let texts = TrayTexts::current();
    let model = collect_model(app_state, &texts)?;

    let mut handles = HashMap::new();
    let mut builder = MenuBuilder::new(app);
    for entry in &model.entries {
        builder = match attach_entry(app, entry, &mut handles)? {
            Some(item) => builder.item(&item),
            None => builder.separator(),
        };
    }
    let menu = builder.build().map_err(menu_error)?;

    update_problem_indicator(app, &texts, &model.status.problems);
    Ok(menu)
}

// ─── 图标圆点和悬停提示 ───────────────────────────────────────────────────────

/// 悬停提示最多几个字（Windows 的提示缓冲区是 128 个 UTF-16 单元，超了会被截掉结尾的 0）。
const MAX_TOOLTIP_CHARS: usize = 100;
/// 圆点颜色（彩色图标用，同画板 `--sys-dot`）。
const PROBLEM_DOT_RGB: [u8; 3] = [0xF7, 0x63, 0x0C];
/// 圆点半径、连同外圈透明边的半径（占图标边长的比例，照画板 24px 图标上 3.3 / 5.1 的圆）。
const DOT_RADIUS: f32 = 0.15;
const DOT_RING_RADIUS: f32 = 0.21;

/// 托盘平时的图标和它是不是 macOS 模板图：macOS 用单色模板图（跟随菜单栏深浅色），读不到时
/// 和其它平台一样用应用图标。读一次缓存起来，出问题 / 恢复时在它上面加减圆点。
///
/// Windows 走一个专门的托盘图：应用图标在 16px 下笔画会糊成一块，
/// 而托盘图标几乎是这个工具唯一的可见形态——它值得单独一份资源。
#[cfg(target_os = "windows")]
fn load_windows_tray(connected: bool) -> Option<Image<'static>> {
    static IDLE: OnceLock<Option<Image<'static>>> = OnceLock::new();
    static CONNECTED: OnceLock<Option<Image<'static>>> = OnceLock::new();
    let slot = if connected { &CONNECTED } else { &IDLE };
    slot.get_or_init(|| {
        // 48px: Windows scales the tray icon, and 48 stays sharp up to 200%.
        let bytes: &[u8] = if connected {
            include_bytes!("../icons/tray/windows/tray-connected-48.png")
        } else {
            include_bytes!("../icons/tray/windows/tray-idle-48.png")
        };
        match Image::from_bytes(bytes) {
            Ok(icon) => Some(icon),
            Err(err) => {
                log::warn!("Failed to load Windows tray icon: {err}");
                None
            }
        }
    })
    .clone()
}

pub fn base_tray_icon(app: &tauri::AppHandle) -> Option<(Image<'static>, bool)> {
    #[cfg(target_os = "macos")]
    {
        static BASE: OnceLock<Option<(Image<'static>, bool)>> = OnceLock::new();
        return BASE
            .get_or_init(|| {
                const ICON_BYTES: &[u8] =
                    include_bytes!("../icons/tray/macos/statusbar_template_3x.png");
                match Image::from_bytes(ICON_BYTES) {
                    Ok(icon) => Some((icon, true)),
                    Err(err) => {
                        log::warn!("Failed to load macOS tray icon: {err}");
                        log::warn!("Falling back to default window icon for tray");
                        None
                    }
                }
            })
            .clone();
    }
    #[cfg(target_os = "windows")]
    {
        // Black until a client actually points at the gateway, green after.
        // Read the live config on every call: the icon is refreshed whenever
        // the tray menu is rebuilt, which is exactly when a switch happened.
        let connected = crate::commands::aimyria::any_client_configured();
        if let Some(icon) = load_windows_tray(connected) {
            return Some((icon, false));
        }
        log::warn!("Failed to load Windows tray icon; using the app icon");
    }
    match app.default_window_icon() {
        Some(icon) => Some((icon.clone().to_owned(), false)),
        None => {
            log::warn!("Failed to get default window icon for tray");
            None
        }
    }
}

/// 出问题时的图标：右下角一个实心圆点，圆点外挖一圈透明边和图形隔开。模板图只看不透明度，
/// 圆点画成黑色、由系统按菜单栏深浅色着色（单色，靠形状区分）；彩色图画橙点。
fn icon_with_problem_dot(icon: &Image<'_>, template: bool) -> Image<'static> {
    let (width, height) = (icon.width(), icon.height());
    let mut rgba = icon.rgba().to_vec();
    if width == 0 || height == 0 || rgba.len() != (width as usize) * (height as usize) * 4 {
        return Image::new_owned(rgba, width, height);
    }
    let size = width.min(height) as f32;
    let dot_radius = size * DOT_RADIUS;
    let ring_radius = size * DOT_RING_RADIUS;
    let (cx, cy) = (width as f32 - ring_radius, height as f32 - ring_radius);
    let color = if template { [0, 0, 0] } else { PROBLEM_DOT_RGB };
    let x0 = (cx - ring_radius - 1.0).max(0.0) as u32;
    let y0 = (cy - ring_radius - 1.0).max(0.0) as u32;
    for y in y0..height {
        for x in x0..width {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let distance = (dx * dx + dy * dy).sqrt();
            // 边缘各留半个像素做抗锯齿。
            let cut = (ring_radius + 0.5 - distance).clamp(0.0, 1.0);
            if cut <= 0.0 {
                continue;
            }
            let i = ((y * width + x) * 4) as usize;
            let fill = (dot_radius + 0.5 - distance).clamp(0.0, 1.0);
            if fill > 0.0 {
                // 圆点（含边缘）整个落在透明圈里，底下原本的像素已经挖掉了。
                rgba[i..i + 3].copy_from_slice(&color);
                rgba[i + 3] = (255.0 * fill).round() as u8;
            } else {
                rgba[i + 3] = (f32::from(rgba[i + 3]) * (1.0 - cut)).round() as u8;
            }
        }
    }
    Image::new_owned(rgba, width, height)
}

/// 悬停提示：平时 `CC Switch`，出问题时 `CC Switch · 需要处理：<问题区第一条>`。额度不算问题。
fn tray_tooltip(texts: &TrayTexts, problems: &[TrayProblem]) -> String {
    match problems.first() {
        None => "AimyriA go".to_string(),
        Some(problem) => truncate_chars(
            &fill(
                texts.tooltip_problem,
                &[("problem", &problem_text(texts, problem))],
            ),
            MAX_TOOLTIP_CHARS,
        ),
    }
}

/// 现在托盘上的（有没有圆点，悬停提示）；`None` = 托盘图标还没建好，还是建图标时的样子。
static APPLIED_INDICATOR: Lazy<Mutex<Option<(bool, String)>>> = Lazy::new(|| Mutex::new(None));

/// 问题区有内容时托盘图标加圆点、悬停提示写第一条问题；问题都消失后换回原图标和提示。只在
/// 变了时才动（Linux 每次换图标都要写临时文件）。
fn update_problem_indicator(app: &tauri::AppHandle, texts: &TrayTexts, problems: &[TrayProblem]) {
    // 启动时第一次建菜单那会儿托盘图标还没建，等下一次重建。
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let has_dot = !problems.is_empty();
    let tooltip = tray_tooltip(texts, problems);
    // 只在锁里比对和记账：换图标要回主线程执行，不能拿着锁等。
    let icon_changed = {
        let mut slot = PROBLEM_INDICATOR.lock().unwrap();
        let changed = *slot != Some(has_dot);
        *slot = Some(has_dot);
        changed
    };
    if icon_changed {
        if let Some((base, template)) = base_tray_icon(app) {
            let icon = if has_dot {
                icon_with_problem_dot(&base, template)
            } else {
                base
            };
            if let Err(e) = tray.set_icon(Some(icon)) {
                log::warn!("[Tray] 更新托盘图标失败: {e}");
            }
            // macOS 上 set_icon 会把模板标记清掉，要再设一次，否则深色菜单栏里是黑图标。
            if template {
                if let Err(e) = tray.set_icon_as_template(true) {
                    log::warn!("[Tray] 恢复模板图标标记失败: {e}");
                }
            }
        }
    }
    // Linux（AppIndicator）不显示悬停提示，设了也无害。
    if let Err(e) = tray.set_tooltip(Some(&tooltip)) {
        log::warn!("[Tray] 更新托盘悬停提示失败: {e}");
    }
}

/// 就地更新各应用行的标题（额度变化时走这条），避免 `set_menu` 关掉用户正开着的菜单。
/// 句柄由上一次 `create_tray_menu` 填充；为空（从未构建过菜单）时无事发生。
fn update_tray_usage_labels(app: &tauri::AppHandle) {
    // Linux（AppIndicator）上 `Submenu::set_text` 不稳，标题会变空（#3385）：一律整菜单重建。
    #[cfg(target_os = "linux")]
    {
        refresh_tray_menu(app);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let Some(app_state) = app.try_state::<AppState>() else {
            return;
        };
        let texts = TrayTexts::current();
        let Ok((snapshots, _)) = collect_snapshots(app_state.inner(), &texts) else {
            return;
        };
        // 先拷出句柄再改标题：`set_text` 要回主线程执行，不能拿着锁等。
        for snapshot in &snapshots {
            let submenu = TRAY_SECTION_SUBMENUS
                .lock()
                .unwrap()
                .get(&snapshot.app)
                .cloned();
            let Some(submenu) = submenu else { continue };
            if let Err(e) = submenu.set_text(app_row_title(&texts, snapshot)) {
                log::debug!("[Tray] 更新{}子菜单标题失败: {e}", snapshot.app.as_str());
            }
        }
    }
}

pub fn refresh_tray_menu(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        match create_tray_menu(app, state.inner()) {
            Ok(new_menu) => {
                if let Some(tray) = app.tray_by_id(TRAY_ID) {
                    if let Err(e) = tray.set_menu(Some(new_menu)) {
                        log::error!("刷新托盘菜单失败: {e}");
                    }
                }
            }
            Err(e) => log::error!("创建托盘菜单失败: {e}"),
        }
    }
}

/// 悬停到托盘图标时：问题区该出现 / 该消失了（路由服务停了、又起来了）、反馈行显示过了或到点了，
/// 就重建菜单。只在变了时重建，菜单还没打开，不会被关掉。
pub fn refresh_tray_if_problems_changed(app: &tauri::AppHandle) {
    static LAST_CHECK: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    {}
    schedule_tray_status_check(app);
}

/// 路由服务启动 / 停止之后：圆点、问题区、「退出」的后果可能要跟着变。在后台线程比对，
/// 变了才重建（调用方可能在异步任务里，也可能正等着主线程）。
pub fn schedule_tray_status_check(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || refresh_tray_if_status_changed(&app));
}

fn refresh_tray_if_status_changed(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let texts = TrayTexts::current();
    let Ok((_, status)) = collect_status(state.inner(), &texts) else {
        return;
    };
}

// ─── 点击 ────────────────────────────────────────────────────────────────────

/// 托盘让主界面去的地方（`tray-navigate` 事件 + `take_tray_navigation` 命令）。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayNavigation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// 设置分组（`routing`）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// `needsRoute`：弹「需要路由」对话框；`add`：开始添加供应商
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
}

impl TrayNavigation {
    fn is_empty(&self) -> bool {
        self.app.is_none() && self.section.is_none()
    }
}

/// 等主界面来取的导航：轻量模式下窗口是新建的，事件发出去时前端可能还没开始监听。
static PENDING_NAVIGATION: Lazy<Mutex<Option<TrayNavigation>>> = Lazy::new(|| Mutex::new(None));

/// 解析 `nav:…` 菜单 id：`nav:main`、`nav:settings:<分组>`、`nav:app:<应用>[:…]`、
/// `nav:add:<应用>`、`nav:needs:<应用>:<供应商 id>`。
fn parse_nav_id(id: &str) -> Option<TrayNavigation> {
    let rest = id.strip_prefix("nav:")?;
    let mut parts = rest.splitn(3, ':');
    let kind = parts.next()?;
    let target = parts.next();
    let tail = parts.next();
    let app = |value: &str| {
        value
            .parse::<AppType>()
            .ok()
            .map(|app| app.as_str().to_string())
    };
    Some(match kind {
        "main" => TrayNavigation::default(),
        "settings" => TrayNavigation {
            section: Some(target?.to_string()),
            ..Default::default()
        },
        "app" => TrayNavigation {
            app: Some(app(target?)?),
            ..Default::default()
        },
        "add" => TrayNavigation {
            app: Some(app(target?)?),
            intent: Some("add".to_string()),
            ..Default::default()
        },
        "needs" => TrayNavigation {
            app: Some(app(target?)?),
            intent: Some("needsRoute".to_string()),
            provider_id: Some(tail.filter(|id| !id.is_empty())?.to_string()),
            ..Default::default()
        },
        _ => return None,
    })
}

/// 主界面取走托盘留下的导航（取一次就清空）。
#[tauri::command]
pub fn take_tray_navigation() -> Option<TrayNavigation> {
    let pending = PENDING_NAVIGATION.lock().unwrap().take();
    pending
}

/// 显示并聚焦主窗口；轻量模式下重建窗口。
pub fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "windows")]
        {
            let _ = window.set_skip_taskbar(false);
        }
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        #[cfg(target_os = "linux")]
        {
            crate::linux_fix::nudge_main_window(window.clone(), "tray-show-main");
        }
        #[cfg(target_os = "macos")]
        {
            apply_tray_policy(app, true);
        }
    } else if crate::lightweight::is_lightweight_mode() {
        if let Err(e) = crate::lightweight::exit_lightweight_mode(app) {
            log::error!("退出轻量模式重建窗口失败: {e}");
        }
    }
}
const MIN_TRAY_USAGE_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

/// 合并多次快速触发的"usage 标题软更新"：批量刷新期间多个 usage 命令
/// 同时成功时，只会产生一次就地 `set_text` 批量调用。走软更新而不是
/// `refresh_tray_menu` 整建，避免用户打开中的菜单被 macOS 系统关闭。
static TRAY_REBUILD_SCHEDULED: AtomicBool = AtomicBool::new(false);

pub fn schedule_tray_refresh(app: &tauri::AppHandle) {
    if TRAY_REBUILD_SCHEDULED.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // 50ms 合窗：让同一轮 React Query / 托盘批量刷新触发的多个写入
        // 共享一次标题更新。
        std::thread::sleep(std::time::Duration::from_millis(50));
        TRAY_REBUILD_SCHEDULED.store(false, Ordering::Release);
        update_tray_usage_labels(&app);
    });
}

/// 并行刷新每个可见应用"在用的那家"的用量；成功 / 失败结果都通过各
/// command 的 write-through 逻辑写入 `UsageCache`，单次标题更新由
/// `schedule_tray_refresh` 做合并。内部 10 秒节流防止鼠标悬停反复进出时
/// 雪崩请求；互斥锁被毒化时以上次状态为准继续推进，不会永久阻塞。
///
/// 刷新面与 `usage_view` 的展示面严格对齐 —— 每次悬停最多发
/// `TRAY_APPS.len()` 个用量查询；按供应商用量开关查询，Codex 托管账号
/// 未保存开关时与卡片一致默认启用。
pub(crate) async fn refresh_all_usage_in_tray(app: &tauri::AppHandle) {
    {
        let now = std::time::Instant::now();
    }

    let Some(app_state) = app.try_state::<AppState>() else {
        return;
    };

    // 与 `create_tray_menu` 保持一致：用户隐藏的 app 不参与外部 API 查询，
    // 避免在未使用的 app 上浪费请求、撞 rate limit 或反复触发鉴权失败日志。
    let visible_apps = crate::settings::get_settings()
        .visible_apps
        .unwrap_or_default();

    let mut usage_futures: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    for app_type in TRAY_APPS.iter() {
        if !visible_apps.is_visible(app_type) {
            continue;
        }

        let app_type_str = app_type.as_str();
        // 只需当前 provider —— by-id 查询避免把整个 app 的 provider 列表加载
        // 进内存（每次悬停的热路径）。
    }
}
