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
use crate::services::profile::{ProfileScope, ProfileService};
use crate::services::provider::ProviderService;
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

/// 应用行前的「已导入」标记：一个实心圆点。
///
/// 用字形而不是文字，是为了不把菜单撑宽 —— 它只占一个字宽，紧跟在应用名后面，
/// 读起来就是那个应用旁边亮了一颗状态灯。真正的文字说明在悬停提示里。
const IMPORTED_MARK: &str = "●";
/// 悬停提示里那句「下一步做什么」。业主的原话是「避免用户点了导入之后感觉没反应
/// 没生效」，所以这句必须说清"已经生效了"和"接下来可以点它"。
const IMPORTED_TOOLTIP: &str = "已导入 · 可去点它";

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
    *FEEDBACK.lock().unwrap() = Some(FeedbackRecord {
        feedback,
        at: std::time::Instant::now(),
        seen: false,
    });
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
    // The "this is where the import landed" marker.
    //
    // The owner's complaint was that after clicking 「导入」 a user cannot tell
    // whether anything happened, or which client it happened to. The tray icon
    // already answered "something is configured" in exactly one bit (black vs
    // green, see `any_client_configured`); this row-level marker answers *which*
    // client, reusing that same bit and therefore the same colour language --
    // deliberately no second notion of "configured", because two definitions of
    // green are worse than one that is occasionally conservative.
    //
    // A filled circle rather than a word: `●` is one glyph wide, survives every
    // font, and reads as a status light next to the app's name. The words go in
    // the hover tooltip, which has room for them.
    if crate::commands::aimyria::client_configured(&snapshot.app) {
        parts.push(IMPORTED_MARK.to_string());
    }
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

/// 悬停提示：平时 `AimyriA go`，出问题时 `AimyriA go · 需要处理：<问题区第一条>`。额度不算问题。
///
/// 另外：至少有一个客户端已导入时，提示里追加一句「已导入 · 可去点它」。
/// 这是需求 8 的可见反馈 —— 托盘圆点/行标记说明"导入了、导到哪家"，
/// 这句话说明"下一步该做什么"。刻意**不用**弹窗或系统气泡：那会打断用户、
/// 会被专注助手静音、且与"启动静默"的产品定位冲突。
fn tray_tooltip(texts: &TrayTexts, problems: &[TrayProblem]) -> String {
    let base = match problems.first() {
        None => "AimyriA go".to_string(),
        Some(problem) => truncate_chars(
            &fill(
                texts.tooltip_problem,
                &[("problem", &problem_text(texts, problem))],
            ),
            MAX_TOOLTIP_CHARS,
        ),
    };
    if crate::commands::aimyria::any_client_configured() {
        // Truncated as a whole afterwards, so a long problem line cannot push the
        // imported notice out of the tooltip.
        return truncate_chars(&format!("{base} · {IMPORTED_TOOLTIP}"), MAX_TOOLTIP_CHARS);
    }
    base
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
            // Skip a repaint that would change nothing.
            //
            // The tray icon was built moments earlier by `TrayIconBuilder::icon(...)`
            // from the *same* source (`base_tray_icon`), so on a healthy startup the
            // "plain" icon is already showing and setting it again is a redundant
            // `Shell_NotifyIcon(NIM_MODIFY)` -- a second repaint of the one piece of UI
            // this tray app always has on screen. That is consistent with the owner's
            // report of something flickering in the notification area at startup, and
            // removing it changes no visible state.
            //
            // Honest limit: this is a code-reasoning change, NOT a measured fix. On
            // this Windows 11 build the taskbar/notification area does not expose its
            // button or icon list to any Win32 probe we could validate -- three
            // separate probes, each caught by a positive control that failed to
            // discriminate. So nobody has visually confirmed that this was the cause,
            // or that this removes it. It is safe (worst case: one fewer repaint) and
            // it is the only candidate that can be touched without risk.
            if has_dot {
                if let Err(e) = tray.set_icon(Some(icon)) {
                    log::warn!("[Tray] 更新托盘图标失败: {e}");
                }
                // macOS 上 set_icon 会把模板标记清掉，要再设一次，否则深色菜单栏里是黑图标。
                if template {
                    if let Err(e) = tray.set_icon_as_template(true) {
                        log::warn!("[Tray] 恢复模板图标标记失败: {e}");
                    }
                }
            } else {
                // No dot: the plain icon is already in place. If it somehow is not,
                // the first problem that appears will correct it, and a missing dot
                // is a cosmetic loss rather than a wrong signal.
                log::debug!("[Tray] plain icon already applied at build time; no redundant set_icon");
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

/// 上一次重建菜单时看到的状态。用来判断这次是否值得重建：菜单没变就不动，
/// 免得把用户正开着的菜单关掉（macOS 上重建菜单会关掉已打开的）。
static LAST_MENU_STATUS: Lazy<Mutex<Option<MenuStatus>>> = Lazy::new(|| Mutex::new(None));

/// 悬停到托盘图标时：问题区该出现 / 该消失了（路由服务停了、又起来了）、反馈行显示过了或到点了，
/// 就重建菜单。只在变了时重建，菜单还没打开，不会被关掉。
pub fn refresh_tray_if_problems_changed(app: &tauri::AppHandle) {
    static LAST_CHECK: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    // 悬停事件可以连续来（鼠标划过），10 秒内只比对一次就够。
    {
        let mut last = LAST_CHECK.lock().unwrap();
        let now = std::time::Instant::now();
        if last.is_some_and(|at| now.duration_since(at) < MIN_TRAY_USAGE_REFRESH_INTERVAL) {
            return;
        }
        *last = Some(now);
    }
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
    // 状态没变就不重建：菜单里只有问题区 / 反馈行 / 「退出」文案会因为状态而变，
    // 其余是随数据库走的，由各自的写入路径触发重建。
    {
        let mut last = LAST_MENU_STATUS.lock().unwrap();
        if last.as_ref() == Some(&status) {
            return;
        }
        *last = Some(status);
    }
    refresh_tray_menu(app);
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
// ─── 点击分发 ─────────────────────────────────────────────────────────────────
//
// 托盘每一项被点之后要做什么，全在这里。菜单是数据（`build_menu_model`），
// 动作是这里的 `plan_menu_action`（也是纯函数，单测直接覆盖），中间那层
// `handle_tray_menu_event` 只负责把 plan 执行掉。
//
// # 为什么拆成「plan + execute」
//
// 曾经只有 execute，且它什么都不做 —— `.on_menu_event(|app, event| {})` 是空的，
// 于是**每一个**托盘条目点了都没反应，不只是「打开官网」。空处理器编译得过、
// 也没有任何测试会失败，所以它能一直留在那里。
//
// 把「这个 id 属于哪种动作」抽成纯函数之后，「有没有入口」就成了一个可以断言的
// 性质：`every_menu_id_plans_to_an_action` 遍历菜单里出现的每种 id，要求它能被
// plan 到一个动作。菜单加一项而忘了接动作，测试会红，而不是安静地失效。

/// 一个托盘条目的动作。
#[derive(Debug, Clone, PartialEq)]
enum MenuAction {
    /// 打开并聚焦主窗口。
    ShowMain,
    /// 进入 / 退出轻量模式。
    ToggleLightweight,
    /// 打开官网。
    OpenWebsite,
    /// 退出应用（走 `app.exit`，触发退出清理）。
    Quit,
    /// 让主界面跳到某个页面 / 开始某个流程。
    Navigate(TrayNavigation),
    /// 切换某个应用的当前供应商。
    SwitchProvider { app: AppType, provider_id: String },
    /// 应用 / 取消某个分组下的项目（Profile）。
    ApplyProfile {
        scope: &'static str,
        profile_id: Option<String>,
    },
}

/// 菜单 id → 动作。`None` 只有一种来源：不可点的行（`label` / 子菜单容器）。
///
/// 判据必须覆盖 `build_menu_model` 生成的**每一种** id 前缀，否则就会出现
/// 「菜单里有、点了没反应」——这正是本函数存在的理由。
fn plan_menu_action(id: &str) -> Option<MenuAction> {
    match id {
        "show_main" => return Some(MenuAction::ShowMain),
        "lightweight_mode" => return Some(MenuAction::ToggleLightweight),
        "open_website" => return Some(MenuAction::OpenWebsite),
        "quit" => return Some(MenuAction::Quit),
        _ => {}
    }

    // 不可点的行：`info:*` 反馈 / 表头，以及 `submenu_*` 子菜单容器本身。
    // 它们由 `TrayEntry::label` / `TrayEntry::Submenu` 生成，`enabled=false`。
    if id.starts_with("info:") || id.starts_with("submenu_") {
        return None;
    }

    // 项目：`profile_none_<scope>` 表示「不使用项目」，`profile_<scope>_<id>` 应用某个项目。
    // 先于 provider 前缀判断，因为两者都以 `profile` 起头的是唯一形式。
    if let Some(rest) = id.strip_prefix("profile_none_") {
        return ProfileScope::parse(rest)
            .ok()
            .map(|scope| MenuAction::ApplyProfile {
                scope: scope.as_str(),
                profile_id: None,
            });
    }
    if let Some(rest) = id.strip_prefix("profile_") {
        // `<scope>_<id>`：scope 本身含连字符（如 `claude-desktop`），所以要按已知
        // 分组名去匹配前缀，而不能简单地 split 一次。
        for scope in ProfileScope::ALL {
            let prefix = format!("{}_", scope.as_str());
            if let Some(profile_id) = rest.strip_prefix(&prefix) {
                if !profile_id.is_empty() {
                    return Some(MenuAction::ApplyProfile {
                        scope: scope.as_str(),
                        profile_id: Some(profile_id.to_string()),
                    });
                }
            }
        }
        return None;
    }

    // 供应商：`prov:<app>:<provider id>`。provider id 可以含 `:`，所以用 splitn(3)。
    if let Some(rest) = id.strip_prefix("prov:") {
        let mut parts = rest.splitn(2, ':');
        let app = parts.next().and_then(|s| s.parse::<AppType>().ok());
        let provider_id = parts.next().filter(|s| !s.is_empty());
        return match (app, provider_id) {
            (Some(app), Some(provider_id)) => Some(MenuAction::SwitchProvider {
                app,
                provider_id: provider_id.to_string(),
            }),
            _ => None,
        };
    }

    // 导航：`nav:main` / `nav:settings:<分组>` / `nav:app:<应用>[:…]` /
    // `nav:add:<应用>` / `nav:needs:<应用>:<供应商>`。尾部被忽略的那几种
    // （`:quota` / `:service` / `:attach` / `:failed`）只是同一页面的不同说法。
    if let Some(navigation) = parse_nav_id(id) {
        return Some(MenuAction::Navigate(navigation));
    }

    None
}

/// 托盘菜单事件入口。`lib.rs` 的 `.on_menu_event` 直接调它。
pub fn handle_tray_menu_event(app: &tauri::AppHandle, event_id: &str) {
    let Some(action) = plan_menu_action(event_id) else {
        // 不可点的行不会派发事件；真收到未知 id 说明菜单与分发器脱节了。
        log::warn!("[Tray] 未处理的菜单事件: {event_id}");
        return;
    };
    log::debug!("[Tray] 菜单事件 {event_id}");
    execute_menu_action(app, action);
}

fn execute_menu_action(app: &tauri::AppHandle, action: MenuAction) {
    match action {
        MenuAction::ShowMain => show_main_window(app),
        MenuAction::ToggleLightweight => {
            let result = if crate::lightweight::is_lightweight_mode() {
                crate::lightweight::exit_lightweight_mode(app)
            } else {
                crate::lightweight::enter_lightweight_mode(app)
            };
            if let Err(e) = result {
                log::error!("[Tray] 轻量模式切换失败: {e}");
            }
        }
        MenuAction::OpenWebsite => {
            if let Err(e) = app.opener().open_url(WEBSITE_URL, None::<String>) {
                log::error!("[Tray] 打开官网失败: {e}");
            }
        }
        MenuAction::Quit => app.exit(0),
        MenuAction::Navigate(navigation) => navigate(app, navigation),
        MenuAction::SwitchProvider { app: app_type, provider_id } => {
            handle_provider_click(app, app_type, &provider_id);
        }
        MenuAction::ApplyProfile { scope, profile_id } => {
            apply_profile_from_tray(app, scope, profile_id.as_deref());
        }
    }
}

/// 产品的官网。托盘「打开官网」与 `product_info` 里那个 `website` 是同一个地址；
/// 上游这里是 ccswitch.io，换成本站自己的。
const WEBSITE_URL: &str = "https://aimyria.com";

/// 让主界面去某个地方：先打开窗口，把导航存下来，再喊一声。
///
/// 存的这一步是必需的：轻量模式下窗口是新建的，`emit` 发出去时前端的
/// `listen()` 还没挂上，事件会被丢掉（Tauri 事件无监听者即丢弃，日志里也不会有）。
/// 前端挂载后会用 `take_tray_navigation` 把这条取走。
fn navigate(app: &tauri::AppHandle, navigation: TrayNavigation) {
    show_main_window(app);
    // 空导航（`nav:main`）只是「打开界面」，没有要跳的地方，不值得惊动前端。
    if navigation.is_empty() {
        return;
    }
    *PENDING_NAVIGATION.lock().unwrap() = Some(navigation);
    if let Err(e) = app.emit("tray-navigate", ()) {
        log::error!("[Tray] 发射 tray-navigate 失败: {e}");
    }
}

/// 托盘里点某家供应商。直连下这家需要路由 → 先弹「需要路由」对话框；否则直接切。
fn handle_provider_click(app: &tauri::AppHandle, app_type: AppType, provider_id: &str) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let state = state.inner();

    // 这家在这一层是否存在 —— 菜单是上一次重建的快照，数据可能已经变了。
    let provider = match ProviderService::list(state, app_type.clone()) {
        Ok(providers) => match providers.get(provider_id) {
            Some(provider) => provider.clone(),
            None => {
                log::warn!(
                    "[Tray] 切换失败：{} 下没有供应商 {provider_id}",
                    app_type.as_str()
                );
                return;
            }
        },
        Err(e) => {
            log::warn!("[Tray] 读取供应商失败: {e}");
            record_switch_failure(&app_type, e.to_string());
            schedule_tray_status_check(app);
            return;
        }
    };
    let name = provider.name.clone();

    // 直连下这家需要路由：先让用户去开路由，别直接切（切了也用不了）。
    if !crate::mode::current::is_proxy(&app_type) && provider_needs_routing(&app_type, &provider) {
        navigate(
            app,
            TrayNavigation {
                app: Some(app_type.as_str().to_string()),
                intent: Some("needsRoute".to_string()),
                provider_id: Some(provider_id.to_string()),
                ..Default::default()
            },
        );
        return;
    }

    match ProviderService::switch(state, app_type.clone(), provider_id) {
        Ok(_) => {
            clear_app_problems(&app_type);
            if let Some(feedback) = switch_feedback(&app_type, TrayMode::Direct, name) {
                record_feedback(app, feedback);
            }
            emit_switched(app, state, &app_type, provider_id);
            schedule_tray_status_check(app);
        }
        Err(e) => {
            log::warn!("[Tray] 切换 {} 到 {provider_id} 失败: {e}", app_type.as_str());
            record_switch_failure(&app_type, e.to_string());
            schedule_tray_status_check(app);
        }
    }
}

/// 切换成功后通知前端：当前供应商变了、这一家的路由开关可能也跟着变了。
///
/// payload 形状与 `commands::profile::emit_profile_apply_events` 一致 ——
/// 前端 `src/lib/api/providers.ts` 的 `provider-switched` 监听依赖它。
fn emit_switched(app: &tauri::AppHandle, state: &AppState, app_type: &AppType, provider_id: &str) {
    let app_str = app_type.as_str();
    let (_, auto_failover_enabled) = state.db.get_proxy_flags_sync(app_str);
    let event_data = serde_json::json!({
        "appType": app_str,
        "proxyEnabled": crate::mode::current::is_proxy(app_type),
        "autoFailoverEnabled": auto_failover_enabled,
        "providerId": provider_id,
    });
    if let Err(e) = app.emit("provider-switched", event_data) {
        log::error!("[Tray] 发射 provider-switched 失败: {e}");
    }
}

/// 托盘里点项目：`profile_id` 为 `None` 表示「不使用项目」。
fn apply_profile_from_tray(app: &tauri::AppHandle, scope: &str, profile_id: Option<&str>) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let state = state.inner();
    let Ok(scope) = ProfileScope::parse(scope) else {
        return;
    };

    let Some(profile_id) = profile_id else {
        if let Err(e) = state.db.set_current_profile_id(scope.as_str(), None) {
            log::warn!("[Tray] 取消项目失败: {e}");
            return;
        }
        if let Err(e) = app.emit(
            "profile-applied",
            serde_json::json!({ "profileId": null, "scope": scope.as_str() }),
        ) {
            log::error!("[Tray] 发射 profile-applied 失败: {e}");
        }
        refresh_tray_menu(app);
        return;
    };

    let desktop_was_mapping = crate::commands::profile::desktop_uses_mapping(state, scope);
    match ProfileService::apply(state, profile_id, scope) {
        Ok(_) => {
            for app_type in scope.apps() {
                clear_app_problems(app_type);
            }
            crate::commands::profile::emit_profile_apply_events(
                app,
                state,
                profile_id,
                scope,
                desktop_was_mapping,
            );
        }
        Err(e) => {
            log::warn!("[Tray] 应用项目 {profile_id} 失败: {e}");
            for app_type in scope.apps() {
                record_switch_failure(app_type, e.to_string());
            }
            schedule_tray_status_check(app);
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

#[cfg(test)]
mod menu_action_tests {
    use super::{plan_menu_action, MenuAction, ProfileScope, TRAY_APPS};
    use crate::app_config::AppType;

    /// 菜单能生成的**每一种**可点 id 都必须在 `plan_menu_action` 里有归宿。
    ///
    /// 这是本模块存在的理由。修复前 `on_menu_event` 是空处理器，于是菜单里每一项
    /// 点了都没反应，而且没有任何测试会失败 —— 空处理器编译得过、菜单模型单测也
    /// 照样绿（那些测试只断言「菜单里有什么」，不断言「点了会怎样」）。
    /// 所以这里逐个 id 断言「有计划到的动作」，菜单新增一项却忘了接动作时测试会红。
    ///
    /// 这里的 id 是照着 `build_menu_model` 的生成处拼的，形状与真实菜单一致：
    /// `provider_id_for` / `problem_id` / `app_entry` / `app_children` /
    /// `profile_none_<scope>` / `profile_<scope>_<id>` / 顶层四项。
    #[test]
    fn every_clickable_id_plans_to_something() {
        // 顶层四项。
        for id in ["show_main", "lightweight_mode", "open_website", "quit"] {
            assert!(
                plan_menu_action(id).is_some(),
                "{id} 是顶层菜单项，必须有计划到的动作"
            );
        }

        // 问题区（`problem_id`）：服务未跑、某应用挂接失败 / 切换失败，以及溢出兜底。
        for id in [
            "nav:settings:routing",
            &format!("nav:app:{}:attach", AppType::Claude.as_str()),
            &format!("nav:app:{}:failed", AppType::Codex.as_str()),
            "nav:main",
        ] {
            assert!(
                matches!(plan_menu_action(id), Some(MenuAction::Navigate(_))),
                "{id} 应解析成导航动作"
            );
        }

        // 每个进托盘的应用下，所有会出现的导航 / 切换 / 项目 id。
        for app in TRAY_APPS {
            let a = app.as_str();
            for id in [
                format!("nav:app:{a}"),
                format!("nav:app:{a}:quota"),
                format!("nav:app:{a}:service"),
                format!("nav:add:{a}"),
                format!("nav:needs:{a}:some-provider"),
            ] {
                assert!(
                    matches!(plan_menu_action(&id), Some(MenuAction::Navigate(_))),
                    "{id} 应解析成导航动作"
                );
            }

            // provider 切换项：id 里含 provider id，provider id 本身可能含 `:`。
            assert!(matches!(
                plan_menu_action(&format!("prov:{a}:some-provider")),
                Some(MenuAction::SwitchProvider { .. })
            ));
            assert!(matches!(
                plan_menu_action(&format!("prov:{a}:vendor:with:colons")),
                Some(MenuAction::SwitchProvider { .. })
            ));
        }

        // 项目：`claude-desktop` 这种带连字符的 scope 是容易被 split 一次而解析错的那种。
        for scope in ProfileScope::ALL {
            let s = scope.as_str();
            for id in [format!("profile_none_{s}"), format!("profile_{s}_proj-1")] {
                assert!(
                    matches!(plan_menu_action(&id), Some(MenuAction::ApplyProfile { .. })),
                    "{id} 应解析成应用项目动作"
                );
            }
        }
    }

    /// 不可点的行必须计划成 `None`，不能被当成动作 —— 否则会把灰字变成按钮。
    #[test]
    fn non_clickable_rows_plan_to_nothing() {
        for id in [
            "info:feedback",
            "info:claude:header",
            "info:claude:failover",
            "info:claude:blocked:some-provider",
            "info:claude:projects",
            "submenu_claude",
        ] {
            assert!(
                plan_menu_action(id).is_none(),
                "{id} 是不可点行 / 子菜单容器，不应有计划到的动作"
            );
        }
    }

    /// 每个 id 的解析结果要具体到正确的目标，不只是「非 None」。
    ///
    /// 「有计划到的动作」还不够：把 `profile_none_<scope>` 解析成 `profile_<scope>_<id>`
    /// 也会通过上面那条断言，但点「不使用项目」会变成「应用项目 X」。
    #[test]
    fn ids_resolve_to_the_right_target() {
        assert_eq!(
            plan_menu_action("profile_none_claude-desktop"),
            Some(MenuAction::ApplyProfile {
                scope: "claude-desktop",
                profile_id: None,
            })
        );
        assert_eq!(
            plan_menu_action("profile_claude-desktop_proj-1"),
            Some(MenuAction::ApplyProfile {
                scope: "claude-desktop",
                profile_id: Some("proj-1".to_string()),
            })
        );
        assert_eq!(
            plan_menu_action("prov:claude-desktop:vendor:x"),
            Some(MenuAction::SwitchProvider {
                app: AppType::ClaudeDesktop,
                provider_id: "vendor:x".to_string(),
            })
        );

        // `nav:needs` 的第三个字段是 provider id，导航要带着它，前端才能定位到那一行。
        match plan_menu_action("nav:needs:codex:vendor:x") {
            Some(MenuAction::Navigate(nav)) => {
                assert_eq!(nav.app.as_deref(), Some("codex"));
                assert_eq!(nav.intent.as_deref(), Some("needsRoute"));
                assert_eq!(nav.provider_id.as_deref(), Some("vendor:x"));
            }
            other => panic!("nav:needs 应解析成带着 provider 的导航，实为 {other:?}"),
        }
    }
}
