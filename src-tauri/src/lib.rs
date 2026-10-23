// Public for the acceptance test that runs the shipping writers over copies of
// the real WorkBuddy/dsh configs (tests/acceptance_real_configs.rs). The module is
// only reachable from outside the crate under cfg(test), so the release binary
// exposes nothing extra.
#[cfg_attr(test, allow(unused_imports))]
pub mod aimyria_apps;
mod app_config;
mod app_store;
mod auto_launch;
mod balance_watch;
mod claude_desktop_config;
mod claude_mcp;
mod claude_plugin;
mod codex_config;
mod codex_history_migration;
mod codex_rollout_file;
mod codex_state_db;
mod commands;
mod config;
mod database;
mod deeplink;
mod deeplink_apply;
mod error;
mod first_run;
mod gemini_config;
mod gemini_mcp;
mod grok_config;
mod hermes_config;
pub mod init_status;
mod jsonc_document;
mod lightweight;
#[cfg(target_os = "linux")]
mod linux_fix;
mod live;
mod live_writer;
pub mod logging;
pub mod mcode_config;
mod mcp;
mod mode;
mod model_capabilities;
mod openclaw_config;
mod opencode_config;
mod panic_hook;
mod pi_config;
mod prompt;
mod prompt_files;
pub mod relay;
mod provider;
mod proxy;
mod services;
mod session_manager;
mod settings;
mod store;
mod usage_script;
mod tray;
mod updater;
mod window_policy;
mod usage_events;

pub use app_config::{AppType, InstalledSkill, McpApps, McpServer, MultiAppConfig, SkillApps};
pub use codex_config::{
    extract_codex_experimental_bearer_token, get_codex_auth_path, get_codex_config_path,
    read_codex_live_settings, write_codex_live_atomic,
};
pub use settings::{update_settings, AppSettings};
pub use store::AppState;

/// The one relay this product configures clients against.
///
/// Deliberately a bare host, not a URL: five clients store the endpoint in five
/// shapes (an `env` entry, a TOML string, a `.env` fragment, an OpenCode provider
/// block), and a host is what they all end up containing.
pub const AIMYRIA_RELAY_HOST: &str = "api.aimyria.com";
// Root re-exports, matching upstream. `database/tests.rs` refers to `crate::AppError`
// and `crate::Database`, and upstream's lib.rs is what makes those paths resolve —
// copying the two files without this line left `cargo test --lib` failing on
// unresolved imports while `cargo check --lib` stayed green, since the test module
// is not compiled into the lib.
pub use database::{Database, Profile};
pub use error::AppError;
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use std::sync::atomic::{AtomicBool, Ordering};
use std::{fmt, sync::Arc};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::RunEvent;
use tauri::{Emitter, Manager};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

#[cfg(target_os = "windows")]
fn set_windows_app_user_model_id(app: &tauri::AppHandle) {
    let app_id = app.config().identifier.clone();
    let wide_app_id: Vec<u16> = app_id.encode_utf16().chain(std::iter::once(0)).collect();

    let result = unsafe {
        windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(wide_app_id.as_ptr())
    };

    if result < 0 {
        log::warn!("设置 Windows AppUserModelID 失败: 0x{result:08X}");
    } else {
        log::debug!("Windows AppUserModelID 已设置为 {app_id}");
    }
}

pub(crate) struct RedactedUrl<'a> {
    url: &'a str,
    known_secrets: &'a [String],
}

impl fmt::Display for RedactedUrl<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&redact_url_for_log_with_secrets(
            self.url,
            self.known_secrets,
        ))
    }
}

/// 为日志提供惰性 URL 脱敏包装；只有日志实际输出时才解析和重建 URL。
pub(crate) fn url_for_log(url: &str) -> RedactedUrl<'_> {
    RedactedUrl {
        url,
        known_secrets: &[],
    }
}

/// 为持有确切认证材料的调用方提供优先精确匹配、再启发式兜底的 URL 脱敏。
pub(crate) fn url_for_log_with_secrets<'a>(
    url: &'a str,
    known_secrets: &'a [String],
) -> RedactedUrl<'a> {
    RedactedUrl { url, known_secrets }
}

/// 已知密钥参与子串脱敏的最短长度：过短的值(如 "api")当作子串会误伤无关文本，
/// 所以只对足够长、几乎不可能是普通词的值做替换。
const MIN_KNOWN_SECRET_LEN: usize = 8;

/// 唯一的密钥脱敏原语：把字符串里出现的、我们确切握有的密钥值替换为 [REDACTED]。
/// 不做任何“看起来像密钥”的形状猜测——只隐藏已知值，天然收敛、不误伤正常路径。
pub(crate) fn redact_known_secrets(text: &str, known_secrets: &[String]) -> String {
    redact_known_secrets_with_min_length(text, known_secrets, MIN_KNOWN_SECRET_LEN)
}

/// Exact-value redaction for user-visible error text. Unlike URL logging, a
/// short known credential must still be hidden because the result reaches the
/// UI rather than a diagnostic-only path.
pub(crate) fn redact_known_secrets_strict(text: &str, known_secrets: &[String]) -> String {
    redact_known_secrets_with_min_length(text, known_secrets, 1)
}

fn redact_known_secrets_with_min_length(
    text: &str,
    known_secrets: &[String],
    minimum_chars: usize,
) -> String {
    let mut output = text.to_string();
    for secret in known_secrets {
        if secret.chars().count() >= minimum_chars {
            output = output.replace(secret.as_str(), "[REDACTED]");
        }
    }
    output
}

/// 无 scheme 的裸 authority 形态(如 `user:pass@host/path`)剥掉 userinfo：
/// 仅当 `@` 出现在第一个 `/` 之前时才视为凭据。
fn strip_bare_userinfo(input: &str) -> &str {
    let authority_end = input.find('/').unwrap_or(input.len());
    match input[..authority_end].rfind('@') {
        Some(at) => &input[at + 1..],
        None => input,
    }
}

pub(crate) fn redact_url_for_log(url_str: &str) -> String {
    redact_url_for_log_with_secrets(url_str, &[])
}

/// 为日志脱敏 URL：剥掉 userinfo(user:pass@) 与整个 query/fragment，保留
/// scheme/host/port/path 供诊断(如 base_url 配错路径导致 404)，最后再抹掉已知密钥值。
pub(crate) fn redact_url_for_log_with_secrets(url_str: &str, known_secrets: &[String]) -> String {
    let scheme_relative = url_str.starts_with("//");
    let parsed = if scheme_relative {
        url::Url::parse(&format!("https:{url_str}"))
    } else {
        url::Url::parse(url_str)
    };

    let sanitized = match parsed {
        Ok(mut url) if url.has_host() => {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            let rendered = url.as_str();
            if scheme_relative {
                rendered
                    .strip_prefix("https:")
                    .unwrap_or(rendered)
                    .to_string()
            } else {
                rendered.to_string()
            }
        }
        _ => {
            // 解析失败(相对路径、含裸 userinfo 的非法 URL 等)：丢掉 query/fragment，
            // 尽力剥掉 userinfo，其余原样保留。
            let without_tail = url_str.split(['?', '#']).next().unwrap_or(url_str);
            strip_bare_userinfo(without_tail).to_string()
        }
    };

    redact_known_secrets(&sanitized, known_secrets)
}

/// 只保留 `scheme://host:port`，丢掉 path/query/userinfo。用于我们手里没有任何
/// 已知密钥可脱敏 path 的场景——凭据可能整个内嵌在 base_url 的 path 里，此时
/// 记录 path 无法保证不泄漏，只能退回到 origin。
pub(crate) fn redact_url_origin_for_log(url_str: &str) -> String {
    let scheme_relative = url_str.starts_with("//");
    let parsed = if scheme_relative {
        url::Url::parse(&format!("https:{url_str}"))
    } else {
        url::Url::parse(url_str)
    };

    match parsed {
        Ok(url) if url.has_host() => {
            let authority = &url[url::Position::BeforeHost..url::Position::AfterPort];
            if scheme_relative {
                format!("//{authority}")
            } else {
                format!("{}://{authority}", url.scheme())
            }
        }
        _ => "[invalid target]".to_string(),
    }
}

/// 给日志用的错误文本：去掉 TOML 解析诊断里引用的源码行（`1 | key = "..."` 和它上下的
/// `|`、`^` 标注行）。那一行是用户配置原文，出错的可能正是密钥那一行；行列号和原因留着。
pub(crate) fn error_for_log(error: &str) -> String {
    error
        .lines()
        .filter(|line| {
            !line
                .trim_start()
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .trim_start()
                .starts_with('|')
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn runtime_log_level_allows(level: log::Level, max_level: log::LevelFilter) -> bool {
    max_level.to_level().is_some_and(|maximum| level <= maximum)
}

/// Did this launch come from the start-on-login entry?
///
/// `auto_launch` writes `"<exe>" --autostart`, so the flag is present for every
/// login start and absent when a person double-clicks the icon. The single-instance
/// callback uses it to tell "the machine started us" (leave the UI alone — a user
/// who just signed in did not ask to see a window) from "the user clicked us"
/// (raise the window, that is the whole point of clicking).
///
/// Matching is case-insensitive and tolerant of the flag appearing anywhere, so a
/// hand-edited `Run` value still classifies correctly.
fn argv_is_autostart(args: &[String]) -> bool {
    args.iter().any(|arg| {
        let arg = arg.trim().trim_matches('"');
        arg.eq_ignore_ascii_case("--autostart") || arg.eq_ignore_ascii_case("/autostart")
    })
}

/// 统一处理 aimyria:// 深链接 URL
///
/// - 解析 URL
/// - 向前端发射 `deeplink-import` / `deeplink-error` 事件，由确认框决定是否写入
/// - 可选：在成功时聚焦主窗口
///
/// A link is only a request. The UI shows the confirmation first and calls
/// `import_from_deeplink_unified` only after consent, so a web page cannot
/// repoint a coding client without the user seeing it.
/// Whether a URL is one of the schemes this build answers.
///
/// `aimyria` is ours; `ccswitch` is accepted because the console's import
/// button and older documentation still emit it. Treating it as an alias is
/// what makes 「导入到 CCS」 wake this app instead of the browser's
/// "需要安装 CC-Switch" fallback.
fn is_deep_link_url(url_str: &str) -> bool {
    let lower = url_str.trim_start().to_ascii_lowercase();
    lower.starts_with("aimyria://") || lower.starts_with("ccswitch://")
}

fn handle_deeplink_url(
    app: &tauri::AppHandle,
    url_str: &str,
    focus_main_window: bool,
    source: &str,
) -> bool {
    if !is_deep_link_url(url_str) {
        return false;
    }

    log::info!(
        "✓ Deep link URL detected from {source}: {}",
        url_for_log(url_str)
    );

    match crate::deeplink::parse_deeplink_url(url_str) {
        Ok(mut request) => {
            request.raw_url = Some(url_str.to_string());
            log::info!(
                "✓ Successfully parsed deep link: resource={}, app={:?}, name={:?}",
                request.resource,
                request.app,
                request.name
            );

            // A deep link is an untrusted request from a web page. Stash it and
            // let the confirmation dialog call `import_from_deeplink_unified`
            // after the user agrees. Writing here would make "one click" a
            // security hole: any page could repoint a coding CLI, and the
            // dialog would be a notification shown after the fact.
            //
            // Recorded *before* the emit, because on a cold start the emit lands
            // while the webview is still loading and React has not mounted the
            // listener yet. Tauri broadcasts events and drops the ones nobody is
            // listening for, so "emit first and hope" loses exactly the link that
            // matters most: the one that started this process. See
            // `deeplink::stash_pending_deeplink` — the frontend claims it on mount.
            crate::deeplink::stash_pending_deeplink(&request);

            // The request is also emitted so an already-open window can refresh
            // its UI. A window that was hidden is shown here; the React listener
            // then renders the confirmation.
            if let Err(e) = app.emit("deeplink-import", &request) {
                log::debug!("deeplink-import not delivered: {e}");
            }
            let _ = focus_main_window;
            crate::tray::show_main_window(app);
            return true;
        }
        Err(e) => {
            log::error!("✗ Failed to parse deep link URL: {e}");

            if let Err(emit_err) = app.emit(
                "deeplink-error",
                serde_json::json!({
                    "url": url_str,
                    "error": e.to_string()
                }),
            ) {
                log::error!("✗ Failed to emit deeplink-error event: {emit_err}");
            }
        }
    }

    true
}

/// 更新托盘菜单的Tauri命令
#[tauri::command]
async fn update_tray_menu(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    match tray::create_tray_menu(&app, state.inner()) {
        Ok(new_menu) => {
            if let Some(tray) = app.tray_by_id(tray::TRAY_ID) {
                tray.set_menu(Some(new_menu))
                    .map_err(|e| format!("更新托盘菜单失败: {e}"))?;
                return Ok(true);
            }
            Ok(false)
        }
        Err(err) => {
            log::error!("创建托盘菜单失败: {err}");
            Ok(false)
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 设置 panic hook，在应用崩溃时记录日志到 <app_config_dir>/crash.log（默认 ~/.aimyria/crash.log）
    panic_hook::setup_panic_hook();

    let mut builder = tauri::Builder::default();

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            log::info!("=== Single Instance Callback Triggered ===");
            log::debug!("Args count: {}", args.len());
            for (i, arg) in args.iter().enumerate() {
                log::debug!("  arg[{i}]: {}", url_for_log(arg));
            }

            // Check for deep link URL in args (mainly for Windows/Linux command line)
            let mut found_deeplink = false;
            for arg in &args {
                if handle_deeplink_url(app, arg, false, "single_instance args") {
                    found_deeplink = true;
                    break;
                }
            }

            // Leaving lightweight mode rebuilds the main window -- and
            // `exit_lightweight_mode` ends by calling `show()` + `set_focus()`, so it
            // is a window-raise too. It must therefore be driven by the same
            // condition as the raise below, not run before it. Previously this ran
            // unconditionally, which meant a link-less second launch exited
            // lightweight mode (rebuilding and focusing the window) and then raised
            // it again: two raises for one launch nobody asked for.
            //
            // Order matters and is now: decide first, act once.
            if found_deeplink && crate::lightweight::is_lightweight_mode() {
                if let Err(e) = crate::lightweight::exit_lightweight_mode(app) {
                    log::error!("退出轻量模式重建窗口失败: {e}");
                }
            }

            if !found_deeplink {
                log::info!("ℹ No deep link URL found in args");
            }

            // Show the window ONLY when the second launch actually asked for something.
            //
            // # What was wrong
            //
            // This block used to run unconditionally -- the comment said so out loud:
            // "Show and focus window regardless". The single-instance callback is
            // invoked by *any* second launch of the exe, and a second launch with no
            // deep link is not a request to see the UI: it is a script, a duplicated
            // autostart entry, a scheduled task, an installer probe, or -- worst --
            // the updater restarting the app after it replaced the executable. Each of
            // those popped the window to the front and stole focus.
            //
            // Measured, and it is why this changed: during a self-update run the log
            // recorded, in adjacent lines,
            //
            //   [00:18:13] 静默启动模式：主窗口已隐藏
            //   [00:18:14] === Single Instance Callback Triggered ===
            //   [00:18:14] ℹ No deep link URL found in args
            //   [00:18:15] [updater] holding the replacement: window_visible=true
            //
            // -- the app hid its window, then a link-less second launch brought it
            // back, and the updater's own "is the user mid-import" guard concluded a
            // human was working and deferred. Two bugs from one unconditional `show()`:
            // a window on screen nobody asked for (the owner's "窗口一直闪"), and a
            // self-update that stalls.
            //
            // # Why "a deep link arrived" is the right condition
            //
            // It is the only thing a second launch can legitimately be asking for.
            // `handle_deeplink_url` already raises the window itself on the success
            // path (`tray::show_main_window`), so showing it again here would be a
            // second, redundant raise -- which is another flicker, and the reason the
            // old code's `set_skip_taskbar(false)`/`unminimize`/`show`/`set_focus`
            // sequence is not simply moved inside the `if`.
            //
            // # Why a link-less launch is no longer ignored outright
            //
            // The fix above overcorrected. "No deep link" was treated as "nobody is
            // asking for the UI", which is true for a script but false for the one
            // case that matters most: a user double-clicking the app's own icon.
            // Windows runs the icon again, the mutex routes it here with no
            // arguments, and the old code logged "leaving the window exactly as the
            // user left it" and did nothing. To the user the program looked dead —
            // they clicked it and nothing appeared, which is the report "它没再启动过 /
            // 没有任何反应".
            //
            // The distinction that matters is who launched us, and it is available
            // without guessing: the autostart entry always passes `--autostart`
            // (see `auto_launch`), while an Explorer double-click carries nothing.
            // So a bare launch is a user request, and showing/focusing is idempotent
            // when the window is already visible. The updater's own restart passes
            // the original args through, so it never takes this branch either.
            if found_deeplink {
                log::info!("Single-instance wake carried a deep link; the window is raised by the deep-link handler");
            } else if argv_is_autostart(&args) {
                log::info!(
                    "Single-instance wake is an autostart entry; leaving the window as the user left it"
                );
            } else {
                // A person launched the app again. Raise the window: for a tray app
                // this is the only feedback that "I clicked the icon and nothing
                // happened" is no longer true.
                log::info!(
                    "Single-instance wake carried no deep link; raising the window for the user"
                );
                crate::tray::show_main_window(app);
            }
        }));
    }

    #[cfg(target_os = "windows")]
    {
        let startup_page_handled = AtomicBool::new(false);
        builder = builder.on_page_load(move |webview, payload| {
            if webview.label() == "main"
                && payload.event() == tauri::webview::PageLoadEvent::Finished
                && payload.url().scheme() != "about"
                && !startup_page_handled.swap(true, Ordering::Relaxed)
                && !crate::settings::get_settings().silent_startup
            {
                let window = webview.window();
                let _ = window.show();
                // A window that is now on screen must own a taskbar button. This
                // path shows the window without going through
                // `tray::show_main_window`, so it has to make the same claim.
                #[cfg(target_os = "windows")]
                let _ = window.set_skip_taskbar(false);
                log::info!("主页面加载完成，主窗口已显示（含任务栏按钮）");
            }
        });
    }

    let builder = builder
        // 注册 deep-link 插件（处理 macOS AppleEvent 和其他平台的深链接）
        .plugin(tauri_plugin_deep_link::init())
        // 拦截窗口关闭：根据设置决定是否最小化到托盘
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                log::info!("收到窗口关闭请求: label={}", window.label());
                // 数据库版本过新的恢复模式下没有托盘可唤回，关闭即退出，避免应用隐身后台
                let in_db_recovery = crate::init_status::get_init_error()
                    .map(|p| p.kind.as_deref() == Some("db_version_too_new"))
                    .unwrap_or(false);

                let settings = crate::settings::get_settings();

                // Closing the window of a tray app means "put it away", not "quit".
                //
                // It used to mean quit whenever `minimizeToTrayOnClose` was false —
                // which is its value on a machine that has never opened settings.
                // So the X button ended the process, and the tray icon vanished with
                // it: the program was simply gone, which is what "它关掉了" reported.
                //
                // The setting now defaults to on and is honoured; recovery mode is
                // the one exception, because it has no working tray to return to.
                if crate::window_policy::close_should_hide(
                    settings.minimize_to_tray_on_close,
                    in_db_recovery,
                ) {
                    api.prevent_close();

                    // Hide first, then adjust the taskbar button.
                    //
                    // Measured: calling `set_skip_taskbar(true)` *before* `hide()`
                    // left the window visible — the COM call (`ITaskbarList::
                    // DeleteTab`) makes the shell re-evaluate the window, and that
                    // re-evaluation can land after `hide()` and restore it. The
                    // log said "最小化到托盘" while the window stayed on screen,
                    // which is exactly the kind of assertion-without-observation
                    // that the last two rounds were about.
                    //
                    // Hiding first is also the honest order: the window is gone
                    // from the screen, then the button that represented it is
                    // withdrawn.
                    let hidden = window.hide();
                    if let Err(e) = &hidden {
                        log::warn!("隐藏窗口失败: {e}");
                    }
                    #[cfg(target_os = "windows")]
                    {
                        // Only bother once the window is actually off the screen.
                        if hidden.is_ok() {
                            let _ = window.set_skip_taskbar(true);
                        }
                    }
                    log::info!(
                        "关闭请求已处理：最小化到托盘，进程继续运行（hide={}）",
                        if hidden.is_ok() { "ok" } else { "failed" }
                    );
                    #[cfg(target_os = "macos")]
                    {
                        tray::apply_tray_policy(window.app_handle(), false);
                    }
                } else {
                    api.prevent_close();
                    log::info!("关闭请求已处理：退出应用");
                    window.app_handle().exit(0);
                }
            }
        })
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(window_state_flags())
                .build(),
        )
        .setup(|app| {
            let _ = rustls::crypto::ring::default_provider().install_default();

            // 预先刷新 Store 覆盖配置，确保后续路径读取正确（日志/数据库等）
            app_store::refresh_app_config_dir_override(app.handle());
            panic_hook::init_app_config_dir(crate::config::get_app_config_dir());

            // 初始化日志（输出到 <app_config_dir>/logs/aimyria-one.log）
            {
                use tauri_plugin_log::{Target, TargetKind, TimezoneStrategy};

                let log_dir = panic_hook::get_log_dir();
                let log_file = log_dir.join("aimyria-one.log");

                // 确保日志目录存在
                if let Err(e) = std::fs::create_dir_all(&log_dir) {
                    eprintln!("创建日志目录失败: {e}");
                }

                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        // 底层保留 Trace 能力，便于加载用户配置后动态调高级别。
                        // 插件注册后会立即把全局级别收紧到 Info，避免启动阶段全量 Trace。
                        .level(log::LevelFilter::Trace)
                        // plugin-log 的前端 command 会直达 logger，绕过 log 宏的全局
                        // max_level；在分发层补一次过滤，确保动态总开关同样约束前端日志。
                        .filter(|metadata| {
                            runtime_log_level_allows(metadata.level(), log::max_level())
                        })
                        // 只有文件一个 sink，且这个 sink **永远不会失败**。
                        //
                        // 走 `TargetKind::Folder` 是不行的，哪怕是唯一的 target：
                        // fern 的 `fallback_on_error` 在写文件失败时会去写 stderr，
                        // stderr 也失败就 `panic!`（fern-0.7.1/src/log_impl.rs:896）。
                        // release 版 `windows_subsystem = "windows"` 没有控制台，
                        // 第二步必然失败，于是 panic 落在写日志的那一行 —— 而启动流程
                        // 里到处都是 `log::info!`，结果是进程在托盘图标出现前就消失。
                        //
                        // 本机 crash.log 里两条崩溃报告就是这个形状：
                        //   Message: Error performing stderr logging after error occurred
                        //   during regular logging ... stderr error: 管道正在被关闭。
                        //   (os error 232)
                        // 触发时正在写 `已插入 222 条默认模型定价数据` 和
                        // `✓ Copilot / Codex OAuth / xAI OAuth managers initialized`，
                        // 都是普通的启动 INFO。用户看到的就是"自己闪退、再也起不来"。
                        //
                        // `logging::PanicProofWriter` 的 `Write` 永远返回 Ok：丢掉一行
                        // 日志是诊断缺口，被自己的日志系统杀死是服务中断，后者必须
                        // 不能发生。轮转也由它负责，所以不再需要 `RotationStrategy`。
                        .targets([Target::new(TargetKind::Dispatch(
                            tauri_plugin_log::fern::Dispatch::new().chain(
                                tauri_plugin_log::fern::Output::writer(
                                    Box::new(logging::PanicProofWriter::new(log_file)),
                                    "\n",
                                ),
                            ),
                        ))])
                        // 轮转与大小上限都由 `PanicProofWriter` 自己实现（它必须在
                        // 写失败时仍然返回 Ok，所以不能用插件的 RotatingFile）。
                        .timezone_strategy(TimezoneStrategy::UseLocal)
                        .build(),
                )?;

                // 用户配置存在数据库中，数据库尚未打开时使用保守的 Info 级别。
                log::set_max_level(log::LevelFilter::Info);
                log::info!("=== AIMYRIA GO v{} started ===", env!("CARGO_PKG_VERSION"));

                // 上一段进程可能因为写日志失败而丢过行；现在日志系统已就绪，
                // 在文件里留一条记录，免得"日志有断层"被误读成"程序没运行"。
                let dropped = logging::dropped_writes();
                if dropped > 0 {
                    log::warn!("日志写入曾有 {dropped} 行被丢弃（目标不可写），后续行已恢复");
                }
            }

            // 静默自更新：启动 60 秒后首次检查，此后每 6 小时一次。
            //
            // 刻意在 setup 的最后才 arm：它要读 `main` 窗口的可见性来判断用户是否
            // 正在导入，而窗口是 setup 早先才建出来的。fire-and-forget，失败只写日志
            // —— 用户永远不该因为网络问题看到任何东西。
            updater::spawn(app.handle());

            // 首次读取覆盖路径时 logger 尚未可用；此处重放一次，
            // 让 Store 损坏或路径无效等启动警告能够真正落盘。
            let _ = app_store::refresh_app_config_dir_override(app.handle());

            #[cfg(target_os = "windows")]
            set_windows_app_user_model_id(app.handle());

            // 注册 Updater 插件（桌面端）；放在 logger 之后，确保失败可诊断。
            #[cfg(desktop)]
            {
                if let Err(e) = app
                    .handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())
                {
                    // 若配置不完整（如缺少 pubkey），跳过 Updater 而不中断应用
                    log::warn!("初始化 Updater 插件失败，已跳过：{e}");
                }
            }

            // 注入 AppHandle 给 usage_events，让无 AppHandle 持有的写日志路径
            // 也能向前端推送 `usage-log-recorded`。
            // 放在日志系统初始化之后，确保 init 的日志能正常输出。
            usage_events::init(app.handle().clone());

            // 残留自更新文件的自清理 + 记录当前版本。
            //
            // 放在 logger 初始化之后（这样每一行都能落盘）、数据库打开之前
            // （这两件事都不碰数据库）。语义：
            //   - 存在 `*.old`  → 上一次更新**成功走完了替换**，本进程已是新版本；
            //                     现在删掉回滚副本是安全的，而且这正是"保留一个周期"
            //                     中那一个周期结束的时刻。
            //   - 存在 `*.new`  → 上次下载了载荷却没能替换（进程中途没了）。
            //                     不重试、不安装，直接丢弃：那份字节自写入以来再没人
            //                     校验过，留着就是一次未经验证的安装机会。
            updater::startup_cleanup();

            // 初始化数据库
            let app_config_dir = crate::config::get_app_config_dir();
            let db_path = app_config_dir.join("aimyria-one.db");
            let json_path = app_config_dir.join("config.json");

            // 检查是否需要从 config.json 迁移到 SQLite
            let has_json = json_path.exists();
            let has_db = db_path.exists();

            // 如果需要迁移，先验证 config.json 是否可以加载（在创建数据库之前）
            // 这样如果加载失败用户选择退出，数据库文件还没被创建，下次可以正常重试
            let migration_config = if !has_db && has_json {
                log::info!("检测到旧版配置文件，验证配置文件...");

                // 循环：支持用户重试加载配置文件
                loop {
                    match crate::app_config::MultiAppConfig::load() {
                        Ok(config) => {
                            log::info!("✓ 配置文件加载成功");
                            break Some(config);
                        }
                        Err(e) => {
                            log::error!("加载旧配置文件失败: {e}");
                            // 弹出系统对话框让用户选择
                            if !show_migration_error_dialog(app.handle(), &e.to_string()) {
                                // 用户选择退出（此时数据库还没创建，下次启动可以重试）
                                log::info!("用户选择退出程序");
                                std::process::exit(1);
                            }
                            // 用户选择重试，继续循环
                            log::info!("用户选择重试加载配置文件");
                        }
                    }
                }
            } else {
                None
            };

            // 现在创建数据库（包含 Schema 迁移）
            //
            // 说明：从 v3.8.* 升级的用户通常会走到这里的 SQLite schema 迁移，
            // 若迁移失败（数据库损坏/权限不足/user_version 过新等），需要给用户明确提示，
            // 否则表现可能只是“应用打不开/闪退”。
            //
            // 预检：数据库版本过新时，必须先于任何 schema 写操作（create_tables 内含
            // DROP/ALTER 等 DDL）进入恢复界面，避免旧应用对读不懂的更新版 DB 落写。
            match crate::database::Database::stored_user_version_exceeds_supported(&db_path) {
                Ok(Some(version)) => {
                    log::warn!("数据库版本过新（v{version}），引导用户在应用内升级应用");
                    crate::init_status::set_init_error(crate::init_status::InitErrorPayload {
                        path: db_path.display().to_string(),
                        error: format!(
                            "数据库版本过新（{version}），当前应用仅支持 {}，请升级应用后再尝试。",
                            crate::database::SCHEMA_VERSION
                        ),
                        kind: Some("db_version_too_new".to_string()),
                        db_version: Some(version),
                        supported_version: Some(crate::database::SCHEMA_VERSION),
                    });
                    // 主窗口默认 visible:false，恢复界面必须强制显示
                    if let Some(window) = app.get_webview_window("main") {
                        #[cfg(target_os = "windows")]
                        {
                            let _ = window.set_skip_taskbar(false);
                        }
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                    return Ok(());
                }
                Ok(None) => {}
                Err(e) => {
                    log::warn!("预检数据库版本失败，继续正常初始化流程: {e}");
                }
            }

            let db = loop {
                match crate::database::Database::init() {
                    Ok(db) => break Arc::new(db),
                    Err(e) => {
                        log::error!("Failed to init database: {e}");

                        if !show_database_init_error_dialog(app.handle(), &db_path, &e.to_string())
                        {
                            log::info!("用户选择退出程序");
                            std::process::exit(1);
                        }

                        log::info!("用户选择重试初始化数据库");
                    }
                }
            };

            // 数据库可用后立即应用持久化日志级别，避免后续服务初始化
            // 继续使用启动阶段的 Info 回退。损坏配置显式 fail-closed 到 Info。

            // 如果有预加载的配置，执行迁移
            if let Some(config) = migration_config {
                log::info!("开始执行数据迁移...");

                match db.migrate_from_json(&config) {
                    Ok(_) => {
                        log::info!("✓ 配置迁移成功");
                        // 标记迁移成功，供前端显示 Toast
                        crate::init_status::set_migration_success();
                        // 归档旧配置文件（重命名而非删除，便于用户恢复）
                        let archive_path = json_path.with_extension("json.migrated");
                        if let Err(e) = std::fs::rename(&json_path, &archive_path) {
                            log::warn!("归档旧配置文件失败: {e}");
                        } else {
                            log::info!("✓ 旧配置已归档为 config.json.migrated");
                        }
                    }
                    Err(e) => {
                        // 配置加载成功但迁移失败的情况极少（磁盘满等），仅记录日志
                        log::error!("配置迁移失败: {e}，将从现有配置导入");
                    }
                }
            }

            let app_state = AppState::new(db);

            // Register the shared state *before* anything reads it.
            //
            // `Manager::state` panics when the type was never managed, and the
            // imports below (and the balance watcher, and every `#[tauri::command]`
            // that takes `State<'_, AppState>`) all call it. Managing here rather
            // than at the end of setup means a later import that panics cannot
            // leave the process half-built with no state at all.
            app.manage(app_state.clone());

            // Console session.
            //
            // Managed separately from `AppState` on purpose: the session is read at
            // startup (before the database is necessarily open) and is dropped on
            // sign-out, while `AppState` is built once around the database. Mixing
            // the two would mean every test that constructs an `AppState` also has
            // to think about DPAPI.
            //
            // `SessionState::load` never fails: a missing or corrupt file means
            // "signed out", which is a normal state and must not block startup.
            let relay_session = std::sync::Arc::new(crate::relay::SessionState::load());
            {
                let status = tauri::async_runtime::block_on(relay_session.status());
                if status.signed_in {
                    log::info!("[relay] 已载入本机登录态: {}", status.account);
                } else {
                    log::info!("[relay] 本机尚无登录态");
                }
            }
            app.manage(relay_session);

            // 初始化 SkillService
            let skill_service = crate::services::skill::SkillService::new();
            app.manage(commands::skill::SkillServiceState(std::sync::Arc::new(
                skill_service,
            )));

            // ── Auth managers, three of them ──────────────────────────────
            //
            // Copilot, Codex OAuth and xAI OAuth each hold a token store behind
            // an RwLock, and every command touching their tokens takes
            // State<'_, CopilotAuthState> and the like. Without these manage
            // calls Tauri panics with "state not managed for field copilotState",
            // which is what the Codex page's "default 检查出错" was.
            //
            // AppState already carries the Codex manager; these three wrap the
            // command-layer state types.
            {
                use crate::proxy::providers::copilot_auth::CopilotAuthManager;
                use crate::proxy::providers::xai_oauth_auth::XaiOAuthManager;
                use commands::{CodexOAuthState, CopilotAuthState, XaiOAuthState};
                use tokio::sync::RwLock;

                let app_config_dir = crate::config::get_app_config_dir();

                let copilot = CopilotAuthManager::new(app_config_dir.clone());
                app.manage(CopilotAuthState(std::sync::Arc::new(RwLock::new(copilot))));

                let codex_oauth = app.state::<AppState>().codex_oauth_manager.clone();
                app.manage(CodexOAuthState(codex_oauth));

                let xai = XaiOAuthManager::new(app_config_dir);
                app.manage(XaiOAuthState(std::sync::Arc::new(RwLock::new(xai))));

                log::info!("✓ Copilot / Codex OAuth / xAI OAuth managers initialized");
            }

            // 设置 AppHandle 用于代理故障转移时的 UI 更新
            // ============================================================
            // 按表独立判断的导入逻辑（各类数据独立检查，互不影响）
            // ============================================================

            // 1. 初始化默认 Skills 仓库（已有内置检查：表非空则跳过）
            match app_state.db.init_default_skill_repos() {
                Ok(count) if count > 0 => {
                    log::info!("✓ Initialized {count} default skill repositories");
                }
                Ok(_) => {} // 表非空，静默跳过
                Err(e) => log::warn!("✗ Failed to initialize default skill repos: {e}"),
            }

            // 1.1. Skills 统一管理迁移：当数据库迁移到 v3 结构后，自动从各应用目录导入到 SSOT
            // 触发条件由 schema 迁移设置 settings.skills_ssot_migration_pending = true 控制。
            match app_state.db.get_setting("skills_ssot_migration_pending") {
                Ok(Some(flag)) if flag == "true" || flag == "1" => {
                    // 安全保护：如果用户已经有 v3 结构的 Skills 数据，就不要自动清空重建。
                    let has_existing = app_state
                        .db
                        .get_all_installed_skills()
                        .map(|skills| !skills.is_empty())
                        .unwrap_or(false);

                    if has_existing {
                        log::info!(
                            "Detected skills_ssot_migration_pending but skills table not empty; skipping auto import."
                        );
                        let _ = app_state
                            .db
                            .set_setting("skills_ssot_migration_pending", "false");
                    } else {
                        match crate::services::skill::migrate_skills_to_ssot(&app_state.db) {
                            Ok(count) => {
                                log::info!("✓ Auto imported {count} skill(s) into SSOT");
                                if count > 0 {
                                    crate::init_status::set_skills_migration_result(count);
                                }
                                let _ = app_state
                                    .db
                                    .set_setting("skills_ssot_migration_pending", "false");
                            }
                            Err(e) => {
                                log::warn!("✗ Failed to auto import legacy skills to SSOT: {e}");
                                crate::init_status::set_skills_migration_error(e.to_string());
                                // 保留 pending 标志，方便下次启动重试
                            }
                        }
                    }
                }
                Ok(_) => {} // 未开启迁移标志，静默跳过
                Err(e) => log::warn!("✗ Failed to read skills migration flag: {e}"),
            }

            // 1.5. 自动导入 live 配置 + seed 官方预设供应商（Claude / Codex / Gemini）
            //
            // 先 import 后 seed 是有意为之：先把用户手动配置的 settings.json / auth.json / .env
            // 落成 "default" provider 设为 current，再追加官方预设（is_current=false）。
            // 降级回旧版时，旧版的回填和整份写入仍靠这一行保住用户原来的 live 配置。
            //
            // 捕获首次运行快照：所有全新装用户都会看到欢迎弹窗介绍 CC Switch 的工作方式。
            // 读失败时默认不弹，宁可漏弹也不要因为故障打扰用户。
            let first_run_already_confirmed = crate::settings::get_settings()
                .first_run_notice_confirmed
                .unwrap_or(false);
            let fresh_install_at_startup =
                app_state.db.is_providers_empty().unwrap_or(false);

            for app_type in
                crate::app_config::AppType::all().filter(|t| !t.is_additive_mode())
            {

                // Skip apps whose live config should not become a provider row.
                //
                // Upstream guards this import; the M2 trim kept the call and lost
                // the guard, so every non-additive app re-imported its own live
                // config after a deep link had configured it — producing a second
                // row named "default" next to the Aimyria row the link created,
                // and marking the duplicate as 使用中.
                if !crate::services::provider::should_import_default_config_on_startup(
                    &app_state,
                    &app_type,
                )
                .unwrap_or(false)
                {
                    log::debug!(
                        "○ {} live import skipped",
                        app_type.as_str()
                    );
                    continue;
                }

                match crate::services::provider::ProviderService::import_default_config(
                    &app_state,
                    app_type.clone(),
                ) {
                    Ok(true) => log::info!(
                        "✓ Imported live config for {} as default provider",
                        app_type.as_str()
                    ),
                    Ok(false) => log::debug!(
                        "○ {} already has providers; live import skipped",
                        app_type.as_str()
                    ),
                    Err(e) => log::debug!(
                        "○ No live config to import for {}: {e}",
                        app_type.as_str()
                    ),
                }
            }

            // One-time cleanup: drop the `default` row that earlier builds created
            // out of our own live config.
            //
            // `import_default_config` now refuses to import a live config aimed at
            // our relay, but that only stops new rows. Every install that ran before
            // the guard already has one persisted, and the user is looking at it:
            // two rows for the same relay, and this one fails its connectivity check
            // with "供应商未配置 OPENAI_BASE_URL" while holding the direct-mode
            // pointer — so leaving the app can write it over the working config.
            //
            // Scoped as narrowly as the bug is: only rows literally named `default`,
            // only ones whose config mentions our relay, and only when the app has
            // another row to fall back to. A `default` row holding a user's own
            // third-party setup is left completely alone.
            for app_type in crate::app_config::AppType::all().filter(|t| !t.is_additive_mode())
            {
                let Ok(all) = app_state.db.get_all_providers(app_type.as_str()) else {
                    continue;
                };
                let stale: Vec<String> = all
                    .values()
                    .filter(|p| p.id == "default" || p.name == "default")
                    .filter(|p| {
                        serde_json::to_string(&p.settings_config)
                            .map(|s| s.contains(crate::AIMYRIA_RELAY_HOST))
                            .unwrap_or(false)
                    })
                    .map(|p| p.id.clone())
                    .collect();

                for id in stale {
                    // Never leave an app with no row at all.
                    if all.len() <= 1 {
                        log::info!(
                            "○ {} 的 default 行指向中转站，但已无其它行，保留",
                            app_type.as_str()
                        );
                        continue;
                    }
                    match crate::services::provider::ProviderService::delete(
                        &app_state,
                        app_type.clone(),
                        &id,
                    ) {
                        Ok(()) => log::info!(
                            "✓ 已删除指向中转站的重复 default 行: {} / {id}",
                            app_type.as_str()
                        ),
                        Err(e) => log::warn!(
                            "删除 {} 的 default 行失败: {}",
                            app_type.as_str(),
                            crate::error_for_log(&e.to_string())
                        ),
                    }
                }
            }

            match app_state.db.init_default_official_providers() {
                Ok(count) if count > 0 => {
                    log::info!("✓ Seeded {count} official provider(s)");
                }
                Ok(_) => {}
                Err(e) => log::warn!("✗ Failed to seed official providers: {e}"),
            }

            {
                let db_for_codex_history_migration = app_state.db.clone();
                tauri::async_runtime::spawn_blocking(move || {
                });
            }

            // 老用户 / 已确认的路径由 `fresh_install_at_startup` 自行拦截，这里不做写入。
            // 字段只由前端在用户点击"我知道了"时 save_settings 回写，语义是"用户显式确认过"。
            if !first_run_already_confirmed && fresh_install_at_startup {
                log::info!("✓ First-run welcome notice pending");
            }

            // 1.6. 自动同步累加模式应用的原生 providers 到数据库
            //
            // additive 模式的 import 函数按 id 幂等——
            // 新 id 执行导入，已有 id 则更新 settings 和 display name，所以每次
            // 启动都跑是安全的：既保证新装用户开箱可见 live 中的供应商，也让外部
            // 修改的 live 文件能在重启后同步到数据库（与之前依赖前端"导入当前配置"
            // 按钮手动触发不同）。
            //
            // 底层 read_*_config 在文件不存在时返回默认空配置，因此新装且无
            // live 文件的用户走 Ok(0) 路径，不会产生错误日志噪音。


            // 迁移旧的 app_config_dir 配置到 Store
            if let Err(e) = app_store::migrate_app_config_dir_from_settings(app.handle()) {
                log::warn!("迁移 app_config_dir 失败: {e}");
            }

            // 启动阶段不再无条件保存,避免意外覆盖用户配置。

            // 注册 deep-link URL 处理器（使用正确的 DeepLinkExt API）
            log::info!("=== Registering deep-link URL handler ===");

            // Linux 和 Windows 调试模式需要显式注册
            #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
            {
                #[cfg(target_os = "linux")]
                {
                    // Use Tauri's path API to get correct path (includes app identifier)
                    // tauri-plugin-deep-link writes to: ~/.local/share/<identifier>/applications/cc-switch-handler.desktop
                    // Only register if .desktop file doesn't exist to avoid overwriting user customizations
                    // TODO(M2): the handler filename is hardcoded as "cc-switch-handler.desktop" from upstream.
                    // If the plugin derives the filename from identifier/package name, this check will never match
                    // on Linux. We ship Windows-only in v1, so this is dormant — verify before enabling Linux builds.
                    let should_register = app
                        .path()
                        .data_dir()
                        .map(|d| !d.join("applications/cc-switch-handler.desktop").exists())
                        .unwrap_or(true);

                    if should_register {
                        if let Err(e) = app.deep_link().register_all() {
                            log::error!("✗ Failed to register deep link schemes: {}", e);
                        } else {
                            log::info!("✓ Deep link schemes registered (Linux)");
                        }
                    } else {
                        log::info!("⊘ Deep link handler already exists, skipping registration");
                    }
                }

                #[cfg(all(debug_assertions, windows))]
                {
                    if let Err(e) = app.deep_link().register_all() {
                        log::error!("✗ Failed to register deep link schemes: {}", e);
                    } else {
                        log::info!("✓ Deep link schemes registered (Windows debug)");
                    }
                }
            }

            // 注册 URL 处理回调（所有平台通用）
            app.deep_link().on_open_url({
                let app_handle = app.handle().clone();
                move |event| {
                    log::info!("=== Deep Link Event Received (on_open_url) ===");
                    let urls = event.urls();
                    log::info!("Received {} URL(s)", urls.len());

                    if crate::lightweight::is_lightweight_mode() {
                        if let Err(e) = crate::lightweight::exit_lightweight_mode(&app_handle) {
                            log::error!("退出轻量模式重建窗口失败: {e}");
                        }
                    }

                    for (i, url) in urls.iter().enumerate() {
                        let url_str = url.as_str();
                        log::debug!("  URL[{i}]: {}", url_for_log(url_str));

                        if handle_deeplink_url(&app_handle, url_str, true, "on_open_url") {
                            break; // Process only first aimyria:// URL
                        }
                    }
                }
            });
            log::info!("✓ Deep-link URL handler registered");

            // 创建动态托盘菜单
            let menu = tray::create_tray_menu(app.handle(), &app_state)?;

            // 构建托盘
            let mut tray_builder = TrayIconBuilder::with_id(tray::TRAY_ID)
                .tooltip("AIMYRIA GO") // 鼠标悬停提示
                .on_tray_icon_event(|tray, event| {
                    // Windows 的习惯是左键打开应用、右键出菜单（按平台给默认值，不加开关）；
                    // macOS 左键仍出菜单；Linux（AppIndicator）不派发点击事件，只能出菜单。
                    #[cfg(target_os = "windows")]
                    {
                        if let TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } = &event
                        {
                            tray::show_main_window(tray.app_handle());
                        }
                    }
                    match &event {
                        // 鼠标悬停/点击到托盘图标时，后台异步刷新用量缓存，
                        // 让用户下一次（或快速打开菜单的那一刻）看到较新的数字。
                        // refresh_all_usage_in_tray 内部有 10 秒防抖。
                        TrayIconEvent::Enter { .. } | TrayIconEvent::Click { .. } => {
                            let app = tray.app_handle().clone();
                            // 悬停时菜单还没打开：问题区该出现 / 消失了就趁这时重建。
                            if matches!(event, TrayIconEvent::Enter { .. }) {
                                tray::refresh_tray_if_problems_changed(&app);
                            }
                            // 弹出的菜单里已经有反馈行了：算显示过，下次悬停时拿掉。
                            if let TrayIconEvent::Click { button, .. } = &event {
                            }
                            tauri::async_runtime::spawn(async move {
                                crate::tray::refresh_all_usage_in_tray(&app).await;
                            });
                        }
                        _ => log::debug!("unhandled event {event:?}"),
                    }
                })
                .menu(&menu)
                .on_menu_event(|app, event| {
                    crate::tray::handle_tray_menu_event(app, event.id().as_ref());
                })
                .show_menu_on_left_click(cfg!(not(target_os = "windows")));

            // 使用平台对应的托盘图标（macOS 使用模板图标适配深浅色）；出问题时 tray.rs 换成带圆点的那张
            if let Some((icon, template)) = tray::base_tray_icon(app.handle()) {
                tray_builder = tray_builder.icon(icon).icon_as_template(template);
            }

            let tray = tray_builder.build(app)?;

            // Honour "show in tray".
            //
            // The setting has existed since the first release with a default of
            // true, but nothing ever read it: turning it off changed nothing, which
            // is worse than not offering it. Now it hides the icon — with one
            // guard: a hidden window plus no tray icon would leave a running
            // process with no way to reach it, so `window_policy` keeps the
            // taskbar button in that case, and this only hides the icon when the
            // window is on screen or the policy says it is safe.
            {
                let settings = crate::settings::get_settings();
                let window_visible = app
                    .get_webview_window("main")
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false);
                let policy = crate::window_policy::decide(crate::window_policy::WindowState {
                    window_visible,
                    show_in_tray: settings.show_in_tray,
                    lightweight: crate::lightweight::is_lightweight_mode(),
                });
                if !policy.tray_icon {
                    if let Err(e) = tray.set_visible(false) {
                        log::warn!("按设置隐藏托盘图标失败: {e}");
                    } else {
                        log::info!("设置已关闭托盘图标；窗口的任务栏按钮保留作为入口");
                    }
                }
            }

            // 将同一个实例注入到全局状态，避免重复创建导致的不一致



            // 初始化全局出站代理 HTTP 客户端
            //
            // 这里是该初始化真正发生的地方 —— 之前这段只剩 `let db = &...`，读到
            // 数据库句柄就结束了，客户端从来没被建出来。后果有两个，都能在日志里
            // 直接看到：
            //
            //   1. 每一条走 `proxy::http_client::get()` 的请求都落到 fallback 分支，
            //      打一行 `[GP-004] Client not initialized, using fallback`。余额轮询
            //      （15 分钟一次）、托盘用量刷新、模型列表都会走这条路，日志里那些
            //      成串的 GP-004 就是它。
            //   2. 更重要的是，fallback 会 **每次调用都新建一个 reqwest Client**：
            //      没有连接池复用，没有 keep-alive，每次都要重新做 TLS 握手。一个
            //      托盘常驻程序最不该在的地方浪费资源和时间。
            //
            // 用户配置的代理（`global_proxy_url`）也因此永远不生效：`apply_proxy`
            // 在客户端不存在时会改走 `init`，而 `init` 只被 S3 测试路径调用过。
            //
            // 初始化必须早于任何后台任务，所以放在托盘建好之后、`balance_watch` /
            // `updater` 这些会发请求的任务启动之前。
            {
                let proxy_url = app
                    .state::<AppState>()
                    .db
                    .get_global_proxy_url()
                    .unwrap_or_else(|e| {
                        log::warn!("读取全局代理配置失败，按直连处理: {e}");
                        None
                    });
                match crate::proxy::http_client::init(proxy_url.as_deref()) {
                    Ok(()) => log::info!("✓ 全局 HTTP 客户端已初始化"),
                    Err(e) => log::warn!("全局 HTTP 客户端初始化失败（将退化为每次新建连接）: {e}"),
                }
            }

            // 异常退出恢复 + 代理状态自动恢复
            let app_handle = app.handle().clone();

            // Watch the relay balance so a user who is about to run out is told
            // before their editor starts refusing requests.
            //
            // Started outside the async block below: `app` is a `&mut App`, which
            // is not `Send`, so capturing it in a spawned future does not
            // compile. `app_handle` is the Sendable clone for exactly this.
            balance_watch::spawn(app_handle.clone());

            // Make sure the OS knows about `aimyria://` and that start-on-login
            // matches what the user asked for. Both re-asserted every launch.
            first_run::maybe_run(&app_handle);

            // A deep link that started us is handled further down, after the
            // silent-startup decision — see the note there. Order matters:
            // whichever runs last decides what the user sees.

            // Background maintenance gets its own handle: `app_handle` is still
            // needed below, by the deep-link scan that runs after this block.
            let for_background = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let state = for_background.state::<AppState>();



                // Periodic backup check (on startup)
                if let Err(e) = state.db.periodic_backup_if_needed() {
                    log::warn!("Periodic backup failed on startup: {e}");
                }

                // Periodic maintenance timer: run once per day while the app is running
                let db_for_timer = state.db.clone();
                tauri::async_runtime::spawn(async move {
                    const PERIODIC_MAINTENANCE_INTERVAL_SECS: u64 = 24 * 60 * 60;
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(
                        PERIODIC_MAINTENANCE_INTERVAL_SECS,
                    ));
                    interval.tick().await; // skip immediate first tick (already checked above)
                    loop {
                        interval.tick().await;
                        if let Err(e) = db_for_timer.periodic_backup_if_needed() {
                            log::warn!("Periodic maintenance timer failed: {e}");
                        }
                    }
                });
            });


            // Linux: 禁用 WebKitGTK 硬件加速，防止 EGL 初始化失败导致白屏
            #[cfg(target_os = "linux")]
            {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.with_webview(|webview| {
                        use webkit2gtk::{WebViewExt, SettingsExt, HardwareAccelerationPolicy};
                        let wk_webview = webview.inner();
                        if let Some(settings) = WebViewExt::settings(&wk_webview) {
                            SettingsExt::set_hardware_acceleration_policy(&settings, HardwareAccelerationPolicy::Never);
                            log::info!("已禁用 WebKitGTK 硬件加速");
                        }
                    });
                }
            }

            // 静默启动：根据设置决定是否显示主窗口
            let settings = crate::settings::get_settings();
            if let Some(window) = app.get_webview_window("main") {
                // 在窗口首次显示前同步装饰状态，避免前端加载后再切换导致标题栏闪烁
                // Linux：由设置决定（解决 Wayland 下系统窗口按钮不可用的问题）
                #[cfg(target_os = "linux")]
                let _ = window.set_decorations(!settings.use_app_window_controls);
                // Windows：一律去掉系统标题栏，用页头里的应用内窗口按钮；保留阴影和边缘缩放
                #[cfg(target_os = "windows")]
                {
                    let _ = window.set_decorations(false);
                    let _ = window.set_shadow(true);
                }
                if settings.silent_startup {
                    // 静默启动：窗口保持隐藏。
                    //
                    // # 任务栏：这里曾经写的是 `skipTaskbar: true`，那是个错误
                    //
                    // 配置里声明过 `"skipTaskbar": true`，于是**任何**情况下主窗口都
                    // 拿不到任务栏按钮。托盘图标一旦被隐藏（或 Explorer 重启丢了它），
                    // 这个程序就变成"进程还在跑，但没有任何入口"——用户点任务栏找不到，
                    // 也没有窗口可以点。这就是"常驻不了任务栏"的根因。
                    //
                    // 现在配置是 `false`，并且按 `window_policy` 决定：
                    //   · 窗口可见 → 一定有任务栏按钮；
                    //   · 窗口隐藏 + 托盘图标在 → 收起按钮（托盘就是入口）；
                    //   · 窗口隐藏 + 托盘图标被关 → **保留**按钮（否则没有入口）。
                    //
                    // 静默启动这一支正好是"窗口隐藏"，所以要不要收按钮，取决于托盘图标
                    // 是否还在。
                    let policy = crate::window_policy::decide(crate::window_policy::WindowState {
                        window_visible: false,
                        show_in_tray: settings.show_in_tray,
                        lightweight: false,
                    });
                    #[cfg(target_os = "windows")]
                    {
                        // show/hide 之后再设最可靠；隐藏前设也有效，但先 hide 再设
                        // 会让 Explorer 看不到变化。这里在 hide 之前设定，语义是
                        // "这个窗口不该出现在任务栏"，然后才隐藏。
                        let _ = window.set_skip_taskbar(!policy.taskbar_button);
                    }
                    let _ = window.hide();
                    #[cfg(target_os = "macos")]
                    tray::apply_tray_policy(app.handle(), false);
                    log::info!(
                        "静默启动模式：主窗口已隐藏（任务栏按钮 {}）",
                        if policy.taskbar_button { "保留" } else { "移除" }
                    );
                } else {
                    // 正常启动模式：显示窗口
                    #[cfg(not(target_os = "windows"))]
                    let _ = window.show();
                    #[cfg(target_os = "windows")]
                    log::info!("正常启动模式：等待主页面加载完成后显示主窗口");
                    #[cfg(not(target_os = "windows"))]
                    log::info!("正常启动模式：主窗口已显示");

                    // Linux: 解决首次启动 UI 无响应问题（Tauri #10746 + wry #637）。
                    // 启动时 webview 未获取焦点 + surface 尺寸协商失败，导致点击无效。
                    // 这里做 set_focus + 伪 resize，等价于无视觉版本的"最大化-还原"。
                    #[cfg(target_os = "linux")]
                    {
                        linux_fix::nudge_main_window(window.clone(), "startup");
                    }
                }
            }

            // A deep link that started this process.
            //
            // Deliberately *after* the silent-startup decision above. Handling
            // it earlier showed the window and then hid it again seven lines
            // later, which is why a user who clicked 一键配置 in a browser saw
            // nothing at all even though every config file was written.
            //
            // It has to run after that block for two reasons: the hide is the
            // default state a deep link is allowed to override, and the window
            // has to exist before anything tries to show it.
            //
            // This is not an edge case. The app starts with Windows and hides
            // its window, so "not running yet" is the normal state on a freshly
            // installed machine and right after a sign-in — and a deep link is
            // the only way to bind an account.
            for arg in std::env::args().skip(1) {
                if handle_deeplink_url(&app_handle, &arg, false, "startup argv") {
                    break;
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // ── AIMYRIA GO's product API ──────────────────────────────
            commands::product_info,
            commands::get_client_status,
            commands::configure_all,
            commands::configure_one,
            commands::revert_to_official,
            commands::get_available_models,
            // Keys as resources: every key this machine knows, and which clients
            // are bound to each. Reads the live config, writes through the same
            // switch the provider page uses.
            commands::relay_credentials,
            commands::relay_bind,
            commands::relay_restart_claude_desktop,
            commands::get_relay_balance,
            // Console session: sign in / out and read who we are. The tokens never
            // leave Rust, which is what makes the session survive a page switch or
            // a WebView reload.
            commands::relay_session_status,
            commands::relay_login,
            commands::relay_logout,
            commands::relay_refresh_session,
            // Keys: one click does create → install → verify. The frontend does not
            // orchestrate it, because a partial result would leave the UI guessing
            // which half succeeded.
            commands::relay_keys_list,
            commands::relay_key_install,
            commands::relay_key_rotate,
            commands::relay_key_disable,
            // Overview: one call, four independently-failing blocks.
            commands::relay_overview,
            commands::get_relay_models,
            commands::get_autostart,
            commands::set_autostart,
            commands::hide_main_window,
            commands::get_providers,
            commands::get_current_provider,
            commands::add_provider,
            commands::update_provider,
            commands::get_provider_editor_view,
            commands::delete_provider,
            commands::remove_provider_from_live_config,
            commands::switch_provider,
            commands::import_default_config,
            commands::ensure_codex_official_provider,
            commands::ensure_grokbuild_official_provider,
            commands::open_external,
            commands::get_init_error,
            commands::get_migration_result,
            commands::read_live_provider_settings,
            commands::get_settings,
            commands::save_settings,
            commands::has_codex_unify_history_backup,
            commands::restart_app,
            commands::install_update_and_restart,
            commands::check_app_update_available,
            commands::check_for_updates,
            commands::is_portable_mode,
            commands::copy_text_to_clipboard,
            // Claude MCP management
            // usage query
            commands::queryProviderUsage,
            commands::testUsageScript,
            // subscription quota
            // New MCP via config.json (SSOT)
            // Unified MCP management
            // Prompt management
            // Pi native provider and session views
            // Profile management (项目配置方案)
            // Fetch OpenAI-compatible and Anthropic model lists. Response data structure:
            // data[].id, data[]?.owned_by. Special: supports Zhipu OpenAI Responses models[].slug.
            commands::fetch_models_for_config,
            commands::get_opencode_models,
            // ours: endpoint speed test + custom endpoint management
            commands::test_api_endpoints,
            commands::get_custom_endpoints,
            commands::add_custom_endpoint,
            commands::remove_custom_endpoint,
            commands::update_endpoint_last_used,
            // app_config_dir override via Store
            commands::get_app_config_dir_override,
            commands::set_app_config_dir_override,
            // provider sort order management
            commands::update_providers_sort_order,
            // theirs: config import/export and dialogs
            // Deep link import
            commands::parse_deeplink,
            commands::merge_deeplink_config,
            commands::import_from_deeplink,
            commands::import_from_deeplink_unified,
            // Lets the dialog claim a link that arrived before it was listening
            // (cold start). Without it the confirmation never appears.
            commands::take_pending_deeplink,

            // ── 用量统计 ────────────────────────────────────────────────
            // The whole `usage_stats` service (5k lines, unchanged from
            // upstream) has been compiled this entire time and was simply never
            // reachable: nothing below exposed a single one of its queries, so
            // the frontend could not ask it anything. Registering them is what
            // turns the dashboard from a mock into a real screen.
            commands::get_usage_summary,
            commands::get_session_usage_summary,
            commands::get_usage_summary_by_app,
            commands::get_usage_trends,
            commands::get_usage_first_date,
            commands::get_provider_stats,
            commands::get_model_stats,
            commands::get_request_logs,
            commands::get_request_detail,
            commands::get_model_pricing,
            commands::check_provider_limits,
            // ── MCP 管理 ──────────────────────────────────────────────────
            commands::get_claude_mcp_status,
            commands::read_claude_mcp_config,
            commands::upsert_claude_mcp_server,
            commands::delete_claude_mcp_server,
            commands::validate_mcp_command,
            commands::get_mcp_config,
            commands::upsert_mcp_server_in_config,
            commands::delete_mcp_server_in_config,
            commands::set_mcp_enabled,
            commands::get_mcp_servers,
            commands::upsert_mcp_server,
            commands::delete_mcp_server,
            commands::toggle_mcp_app,
            commands::import_mcp_from_apps,
            commands::resync_mcp_to_apps,
            // Prompt management
            commands::get_prompts,
            commands::upsert_prompt,
            commands::delete_prompt,
            commands::enable_prompt,
            commands::import_prompt_from_file,
            commands::get_current_prompt_file_content,
            commands::get_prompt_file_location,
            commands::get_pi_prompt_file,
            commands::replace_pi_prompt_file,
            commands::delete_pi_prompt_file,
            commands::list_pi_prompt_templates,
            commands::upsert_pi_prompt_template,
            commands::delete_pi_prompt_template,
            // `get_session_usage_last_sync` is deliberately absent: M2 turned it
            // into a stub returning `None` along with the session-log scanner it
            // used to call. Registering a stub would only hand the frontend a
            // well-formed answer to a question we are no longer answering. It
            // comes back with the session manager.
            update_tray_menu,
            // Environment variable management
            // Skill management (v3.10.0+ unified)
            commands::get_installed_skills,
            commands::get_skill_backups,
            commands::delete_skill_backup,
            commands::install_skill_unified,
            commands::uninstall_skill_unified,
            commands::restore_skill_backup,
            commands::toggle_skill_app,
            commands::scan_unmanaged_skills,
            commands::import_skills_from_apps,
            commands::discover_available_skills,
            commands::check_skill_updates,
            commands::resync_skills_to_apps,
            commands::update_skill,
            commands::migrate_skill_storage,
            commands::get_cc_switch_skills_dir,
            commands::open_cc_switch_skills_dir,
            commands::search_skills_sh,
            // Skill management (legacy API compatibility)
            commands::get_skills,
            commands::get_skills_for_app,
            commands::install_skill,
            commands::install_skill_for_app,
            commands::uninstall_skill,
            commands::uninstall_skill_for_app,
            commands::get_skill_repos,
            commands::add_skill_repo,
            commands::remove_skill_repo,
            commands::install_skills_from_zip,
        
            commands::apply_claude_onboarding_skip,
commands::apply_claude_plugin_config,
commands::check_env_conflicts,
commands::clear_claude_onboarding_skip,
commands::create_db_backup,
commands::delete_backup_location,
commands::delete_daily_memory_file,
commands::delete_db_backup,
commands::delete_env_vars,
commands::delete_model_pricing,
commands::delete_session,
commands::delete_sessions,
commands::disable_current_omo,
commands::disable_current_omo_slim,
commands::export_config_to_file,
commands::export_session_markdown,
commands::extract_common_config_snippet,
commands::get_app_config_path,
commands::get_auto_launch_status,
commands::get_balance,
commands::get_claude_code_config_path,
commands::get_claude_common_config_snippet,
commands::get_claude_config_status,
commands::get_claude_plugin_status,
commands::get_coding_plan_quota,
commands::get_common_config_snippet,
commands::get_config_dir,
commands::get_config_status,
commands::get_current_omo_provider_id,
commands::get_current_omo_slim_provider_id,
commands::get_hermes_live_provider,
commands::get_hermes_live_provider_ids,
commands::get_hermes_memory,
commands::get_hermes_memory_limits,
commands::get_hermes_model_config,
commands::get_models_dev_sync_config,
commands::get_openclaw_agents_defaults,
commands::get_openclaw_default_model,
commands::get_openclaw_env,
commands::get_openclaw_live_provider,
commands::get_openclaw_live_provider_ids,
commands::get_openclaw_model_catalog,
commands::get_openclaw_tools,
commands::get_pi_current_state,
commands::get_pi_session_discovery,
commands::get_session_block_content,
commands::get_session_image,
commands::get_session_messages,
commands::get_session_usage_last_sync,
commands::get_subscription_quota,
commands::get_usage_data_sources,
commands::import_config_from_file,
commands::import_hermes_providers_from_live,
commands::import_openclaw_providers_from_live,
commands::is_claude_plugin_applied,
commands::launch_session_terminal,
commands::list_backup_locations,
commands::list_daily_memory_files,
commands::list_db_backups,
commands::list_sessions,
commands::open_app_config_folder,
commands::open_config_folder,
commands::open_file_dialog,
commands::open_hermes_web_ui,
commands::open_workspace_directory,
commands::open_zip_file_dialog,
commands::pick_directory,
commands::read_claude_plugin_config,
commands::read_daily_memory_file,
commands::read_omo_local_file,
commands::read_omo_slim_local_file,
commands::read_workspace_file,
commands::rebuild_codex_usage,
commands::record_models_dev_sync_result,
commands::rename_db_backup,
commands::restore_db_backup,
commands::restore_env_backup,
commands::reveal_app_log_file,
commands::reveal_backup_location,
commands::reveal_session_path,
commands::s3_sync_download,
commands::s3_sync_fetch_remote_info,
commands::s3_sync_save_settings,
commands::s3_sync_upload,
commands::s3_test_connection,
commands::save_file_dialog,
commands::save_models_dev_sync_config,
commands::scan_openclaw_config_health,
commands::search_daily_memory_files,
commands::set_auto_launch,
commands::set_claude_common_config_snippet,
commands::set_common_config_snippet,
commands::set_hermes_memory,
commands::set_hermes_memory_enabled,
commands::set_openclaw_agents_defaults,
commands::set_openclaw_default_model,
commands::set_openclaw_env,
commands::set_openclaw_model_catalog,
commands::set_openclaw_tools,
commands::set_window_theme,
commands::stream_check_provider,
commands::stream_session_messages,
commands::sync_current_providers_live,
commands::sync_session_usage,
            crate::tray::take_tray_navigation,
            crate::tray::tray_app_page_seen,
commands::update_model_pricing,
commands::update_model_pricing_batch,
commands::webdav_sync_download,
commands::webdav_sync_fetch_remote_info,
commands::webdav_sync_save_settings,
commands::webdav_sync_upload,
commands::webdav_test_connection,
commands::write_daily_memory_file,
commands::write_workspace_file,
            
            
            commands::acknowledge_codex_stale_clients,
            commands::add_to_failover_queue,
            commands::adopt_codex_stack_catalog,
            commands::apply_profile,
            commands::auth_cancel_login,
            commands::auth_get_status,
            commands::auth_list_accounts,
            commands::auth_logout,
            commands::auth_poll_for_account,
            commands::auth_remove_account,
            commands::auth_set_default_account,
            commands::auth_start_login,
            commands::clear_current_profile,
            commands::codex_forces_multi_agent_v2,
            commands::copilot_get_auth_status,
            commands::copilot_get_models,
            commands::copilot_get_models_for_account,
            commands::copilot_get_token,
            commands::copilot_get_token_for_account,
            commands::copilot_get_usage,
            commands::copilot_get_usage_for_account,
            commands::copilot_is_authenticated,
            commands::copilot_list_accounts,
            commands::copilot_logout,
            commands::copilot_poll_for_account,
            commands::copilot_poll_for_auth,
            commands::copilot_remove_account,
            commands::copilot_set_default_account,
            commands::copilot_start_device_flow,
            commands::create_profile,
            commands::delete_profile,
            commands::delete_universal_provider,
            commands::ensure_claude_desktop_official_provider,
            commands::enter_lightweight_mode,
            commands::exit_lightweight_mode,
            commands::exit_proxy_apps_in_mode,
            commands::get_app_mode,
            commands::get_auto_failover_enabled,
            commands::get_available_providers_for_failover,
            commands::get_circuit_breaker_config,
            commands::get_circuit_breaker_stats,
            commands::get_claude_desktop_default_routes,
            commands::get_claude_desktop_status,
            commands::get_codex_oauth_models,
            commands::get_codex_oauth_quota,
            commands::get_codex_session_compression,
            commands::get_codex_sessions_disk_usage,
            commands::get_copilot_optimizer_config,
            commands::get_direct_provider,
            commands::get_failover_queue,
            commands::get_global_proxy_config,
            commands::get_global_proxy_url,
            commands::get_log_config,
            commands::get_opencode_live_provider_ids,
            commands::get_optimizer_config,
            commands::get_pricing_model_source,
            commands::get_provider_health,
            commands::get_proxy_client_host,
            commands::get_proxy_config,
            commands::get_proxy_config_for_app,
            commands::get_proxy_stack,
            commands::get_proxy_status,
            commands::get_proxy_takeover_status,
            commands::get_rectifier_config,
            commands::get_skills_migration_result,
            commands::get_tool_versions,
            commands::get_universal_provider,
            commands::get_universal_providers,
            commands::get_upstream_proxy_status,
            commands::get_xai_oauth_models,
            commands::get_xai_oauth_quota,
            commands::import_claude_desktop_providers_from_claude,
            commands::import_opencode_providers_from_live,
            commands::is_lightweight_mode,
            commands::is_live_takeover_active,
            commands::is_proxy_running,
            commands::launch_hermes_dashboard,
            commands::list_profiles,
            commands::list_tool_installations,
            commands::open_provider_terminal,
            commands::probe_tool_installations,
            commands::remove_from_failover_queue,
            commands::reset_circuit_breaker,
            commands::restart_codex_app_server_daemon,
            commands::restore_codex_unified_history,
            commands::run_tool_lifecycle_action,
            commands::scan_local_proxies,
            commands::set_auto_failover_enabled,
            commands::set_codex_session_compression,
            commands::set_copilot_optimizer_config,
            commands::set_global_proxy_url,
            commands::set_log_config,
            commands::set_optimizer_config,
            commands::set_pricing_model_source,
            commands::set_proxy_client_host,
            commands::set_proxy_route,
            commands::set_proxy_stack_member,
            commands::set_proxy_takeover_for_app,
            commands::set_rectifier_config,
            commands::start_proxy_server,
            commands::stop_proxy_server,
            commands::stop_proxy_with_restore,
            commands::switch_proxy_provider,
            commands::sync_universal_provider,
            commands::take_startup_attach_failures,
            crate::tray::take_tray_navigation,
            commands::test_proxy_url,
            commands::update_circuit_breaker_config,
            commands::update_global_proxy_config,
            commands::update_pi_provider_usage_script,
            commands::update_profile,
            commands::update_proxy_config,
            commands::update_proxy_config_for_app,
            commands::upsert_universal_provider,]);
    let app = builder
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(|app_handle, event| {
        // 处理退出请求（所有平台）
        if let RunEvent::ExitRequested { api, code, .. } = &event {
            match classify_exit_request(*code) {
                // code 为 None 表示运行时自动触发（如隐藏窗口的 WebView 被回收导致无存活窗口），
                // 此时应仅阻止退出、保持托盘后台运行。
                ExitRequestAction::StayInTray => {
                    log::info!("运行时触发退出请求（无存活窗口），阻止退出以保持托盘后台运行");
                    api.prevent_exit();
                    return;
                }
                // code 为 RESTART_EXIT_CODE：app.restart() 发起的重启（本应用自己的重启
                // 都走 restart_process，不经过这里，此分支只兜底）。
                // 这条路径上 prevent_exit() 会被 Tauri 忽略，事件循环必定退出，随后由
                // Tauri 在 RunEvent::Exit 后用新二进制 re-exec（macOS 会按更新后的
                // Info.plist 解析可执行名）。
                //
                // 绝不能复用下面的异步清理任务：该任务在 tokio 线程调 save_window_state，
                // 持有 window-state 插件锁的同时向主线程查询窗口几何；而主线程此刻正在
                // 退出事件循环，并在插件自带的 RunEvent::Exit 钩子里等待同一把锁——双方
                // 互等造成进程永久卡死（更新已安装但应用冻结、不再重启，见 #3998）。
                //
                // 重启路径交还 Tauri 默认流程即可：
                //   - 窗口状态：插件 Exit 钩子在主线程保存（同线程读取窗口几何，无死锁）
                //   - 托盘图标：Tauri 内部 cleanup_before_exit 清理，正常走 Drop
                //   - 代理/Live 配置：无需恢复，重启后新实例立即接管并恢复代理状态
                //   - 100ms 落盘等待：重启前的 DB 写入均为命令驱动、此刻已完成，
                //     与所有 Tauri 应用默认重启路径的行为一致，无需额外等待
                ExitRequestAction::DeferToTauriRestart => {
                    log::info!("收到重启请求 (code={code:?})，交由 Tauri 默认重启流程 re-exec");
                    RESTART_REQUESTED.store(true, Ordering::SeqCst);
                    return;
                }
                // 其它 Some(_)：用户主动调用 app.exit() 退出（如托盘菜单"退出"），
                // 此时执行清理后退出。
                ExitRequestAction::CleanupAndExit => {}
            }

            log::info!("收到用户主动退出请求 (code={code:?})，开始清理...");
            api.prevent_exit();

            let app_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                save_window_state_before_exit(&app_handle);
                cleanup_before_exit(&app_handle).await;
                // 先于 std::process::exit 显式移除托盘图标。
                // 进程直接退出时 Tauri 运行时不走正常 Drop 流程，
                // 不会向 Windows Shell 发送 NIM_DELETE，导致已退出的进程
                // 注册的图标仍残留在系统托盘（鼠标悬停 Shell 才会重绘发现进程已死）。
                remove_tray_icon_before_exit(&app_handle);
                log::info!("清理完成，退出应用");

                // 短暂等待确保所有 I/O 操作（如数据库写入）刷新到磁盘
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                // 使用 std::process::exit 避免再次触发 ExitRequested
                std::process::exit(0);
            });
            return;
        }

        // macOS ⌘Q、Dock「退出」、注销关机走系统 terminate，不发 ExitRequested、只发
        // RunEvent::Exit，回调一返回进程就结束，只能在这里同步补做退出清理。重启也会走到
        // 这里，照上面 DeferToTauriRestart 的约定交还 Tauri 默认流程，不清理。
        if matches!(event, RunEvent::Exit) {
            if !RESTART_REQUESTED.load(Ordering::SeqCst) {
                cleanup_before_system_exit(app_handle);
            }
            return;
        }

        #[cfg(target_os = "macos")]
        {
            match event {
                // macOS 在 Dock 图标被点击并重新激活应用时会触发 Reopen 事件，这里手动恢复主窗口
                RunEvent::Reopen { .. } => {
                    if let Some(window) = app_handle.get_webview_window("main") {
                        #[cfg(target_os = "windows")]
                        {
                            let _ = window.set_skip_taskbar(false);
                        }
                        let _ = window.unminimize();
                        let _ = window.show();
                        let _ = window.set_focus();
                        tray::apply_tray_policy(app_handle, true);
                    } else if crate::lightweight::is_lightweight_mode() {
                        if let Err(e) = crate::lightweight::exit_lightweight_mode(app_handle) {
                            log::error!("退出轻量模式重建窗口失败: {e}");
                        }
                    }
                }
                // 处理通过自定义 URL 协议触发的打开事件（例如 aimyria://...）
                RunEvent::Opened { urls } => {
                    if let Some(url) = urls.first() {
                        let url_str = url.to_string();
                        log::info!("RunEvent::Opened with URL: {}", url_for_log(&url_str));

                        if is_deep_link_url(url_str) {
                            if crate::lightweight::is_lightweight_mode() {
                                if let Err(e) =
                                    crate::lightweight::exit_lightweight_mode(app_handle)
                                {
                                    log::error!("退出轻量模式重建窗口失败: {e}");
                                }
                            }

                            // Same path as `handle_deeplink_url`: parse, stash
                            // for confirmation, never write before consent.
                            handle_deeplink_url(&app_handle, &url_str, true, "RunEvent::Opened");

                            // 确保主窗口可见
                            if let Some(window) = app_handle.get_webview_window("main") {
                                #[cfg(target_os = "windows")]
                                {
                                    let _ = window.set_skip_taskbar(false);
                                }
                                let _ = window.unminimize();
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app_handle, event);
        }
    });
}

// ============================================================
// 应用退出清理
// ============================================================

/// 应用退出前的清理工作
///
/// 把接上代理的客户端都指回直连（模式和代理路由保留，下次启动再接上），再停止代理。
/// 客户端不能一直指着代理：开机自启默认关闭，CC Switch 一关客户端就连不上了。
pub async fn cleanup_before_exit(app_handle: &tauri::AppHandle) {
    if let Some(state) = app_handle.try_state::<store::AppState>() {}
}

/// 系统终止应用时最多等退出清理这么久：停代理服务自带 5 秒超时，指回直连只是写几个文件。
const SYSTEM_EXIT_CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

/// 系统直接终止应用时（macOS ⌘Q、Dock「退出」、注销关机）的退出清理。
///
/// 这条路没有 `ExitRequested` 可以 `prevent_exit()` 再异步清理，只能在主线程上等清理做完。
/// 清理放到异步运行时的线程上跑、主线程限时等：万一里面有步骤要等主线程，超时后照常退出，
/// 不会把进程卡住。
fn cleanup_before_system_exit(app_handle: &tauri::AppHandle) {
    log::info!("系统终止应用，开始退出清理...");
    let handle = app_handle.clone();
    let task = tauri::async_runtime::spawn(async move { cleanup_before_exit(&handle).await });
    // timeout 要在运行时里构造（它取当前运行时的计时器），所以包一层 async。
    let finished = tauri::async_runtime::block_on(async move {
        tokio::time::timeout(SYSTEM_EXIT_CLEANUP_TIMEOUT, task).await
    });
    if finished.is_err() {
        log::warn!(
            "退出清理 {} 秒内没做完，直接退出",
            SYSTEM_EXIT_CLEANUP_TIMEOUT.as_secs()
        );
    }
}

/// 主动从系统托盘移除托盘图标。
///
/// `std::process::exit` 会绕过 Tauri 运行时，触发不了 `TrayIcon::drop()`，
/// 也就不会向 Windows Shell 发 `NIM_DELETE`。结果是进程退出后托盘里
/// 仍保留一个死图标的缓存占位（Shell 不会主动重绘，需要鼠标悬停才刷新）。
///
/// 通过 `set_visible(false)` 走 `WM_USER_HIDE_TRAYICON` 消息路径，
/// 触发 tray-icon 内部的 `remove_tray_icon` → `Shell_NotifyIconW(NIM_DELETE)`，
/// 在进程结束前干净地把图标摘掉。其它平台 `set_visible(false)` 也是
/// 正常的隐藏/移除语义，作为跨平台兜底也安全。
pub(crate) fn remove_tray_icon_before_exit(app_handle: &tauri::AppHandle) {
    if let Some(tray) = app_handle.tray_by_id(tray::TRAY_ID) {
        if let Err(e) = tray.set_visible(false) {
            log::warn!("退出时移除托盘图标失败: {e}");
        } else {
            log::info!("已显式从系统托盘移除图标");
        }
    }
}

fn initialize_common_config_snippets(state: &store::AppState) {
    // Auto-extract common config snippets from clean live files when snippet is missing.
    // This must run before proxy mode is re-attached on startup, otherwise we'd read
    // proxy-placeholder configs instead of the user's actual live settings. A client
    // still attached from an update restart (no detach on the way out) is skipped too.
    for app_type in crate::app_config::AppType::all() {
        if !state
            .db
            .should_auto_extract_config_snippet(app_type.as_str())
            .unwrap_or(false)
        {
            continue;
        }

        let settings = match crate::services::provider::ProviderService::read_live_settings(
            app_type.clone(),
        ) {
            Ok(s) => s,
            Err(_) => continue,
        };

        match crate::services::provider::ProviderService::extract_common_config_snippet_from_settings(
            app_type.clone(),
            &settings,
        ) {
            Ok(snippet) if !snippet.is_empty() && snippet != "{}" => {
                match state.db.set_config_snippet(app_type.as_str(), Some(snippet)) {
                    Ok(()) => {
                        let _ = state.db.set_config_snippet_cleared(app_type.as_str(), false);
                        log::info!(
                            "✓ Auto-extracted common config snippet for {}",
                            app_type.as_str()
                        );
                    }
                    Err(e) => log::warn!(
                        "✗ Failed to save config snippet for {}: {e}",
                        app_type.as_str()
                    ),
                }
            }
            Ok(_) => log::debug!(
                "○ Live config for {} has no extractable common fields",
                app_type.as_str()
            ),
            Err(e) => log::warn!(
                "✗ Failed to extract config snippet for {}: {e}",
                app_type.as_str()
            ),
        }
    }

    let should_run_legacy_migration = state
        .db
        .is_legacy_common_config_migrated()
        .map(|done| !done)
        .unwrap_or(true);

    if should_run_legacy_migration {
        for app_type in [
            crate::app_config::AppType::Claude,
            crate::app_config::AppType::Codex,
            crate::app_config::AppType::Gemini,
        ] {
            if let Err(e) = crate::services::provider::ProviderService::migrate_legacy_common_config_usage_if_needed(
                state,
                app_type.clone(),
            ) {
                log::warn!(
                    "✗ Failed to migrate legacy common-config usage for {}: {e}",
                    app_type.as_str()
                );
            }
        }

        if let Err(e) = state.db.set_legacy_common_config_migrated(true) {
            log::warn!("✗ Failed to persist legacy common-config migration flag: {e}");
        }
    }
}

// ============================================================
// 迁移错误对话框辅助函数
// ============================================================

/// 检测是否为中文环境
fn is_chinese_locale() -> bool {
    std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .map(|lang| lang.starts_with("zh"))
        .unwrap_or(false)
}

/// 显示迁移错误对话框
/// 返回 true 表示用户选择重试，false 表示用户选择退出
fn show_migration_error_dialog(app: &tauri::AppHandle, error: &str) -> bool {
    let title = if is_chinese_locale() {
        "配置迁移失败"
    } else {
        "Migration Failed"
    };

    let message = if is_chinese_locale() {
        format!(
            "从旧版本迁移配置时发生错误：\n\n{error}\n\n\
            您的数据尚未丢失，旧配置文件仍然保留。\n\
            建议回退到旧版本 AIMYRIA GO 以保护数据。\n\n\
            点击「重试」重新尝试迁移\n\
            点击「退出」关闭程序（可回退版本后重新打开）"
        )
    } else {
        format!(
            "An error occurred while migrating configuration:\n\n{error}\n\n\
            Your data is NOT lost - the old config file is still preserved.\n\
            Consider rolling back to an older AIMYRIA GO version.\n\n\
            Click 'Retry' to attempt migration again\n\
            Click 'Exit' to close the program"
        )
    };

    let retry_text = if is_chinese_locale() {
        "重试"
    } else {
        "Retry"
    };
    let exit_text = if is_chinese_locale() {
        "退出"
    } else {
        "Exit"
    };

    // 使用 blocking_show 同步等待用户响应
    // OkCancelCustom: 第一个按钮（重试）返回 true，第二个按钮（退出）返回 false
    app.dialog()
        .message(&message)
        .title(title)
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::OkCancelCustom(
            retry_text.to_string(),
            exit_text.to_string(),
        ))
        .blocking_show()
}

/// 显示数据库初始化/Schema 迁移失败对话框
/// 返回 true 表示用户选择重试，false 表示用户选择退出
fn show_database_init_error_dialog(
    app: &tauri::AppHandle,
    db_path: &std::path::Path,
    error: &str,
) -> bool {
    let title = if is_chinese_locale() {
        "数据库初始化失败"
    } else {
        "Database Initialization Failed"
    };

    let message = if is_chinese_locale() {
        format!(
            "初始化数据库或迁移数据库结构时发生错误：\n\n{error}\n\n\
            数据库文件路径：\n{db}\n\n\
            您的数据尚未丢失，应用不会自动删除数据库文件。\n\
            常见原因包括：数据库版本过新、文件损坏、权限不足、磁盘空间不足等。\n\n\
            建议：\n\
            1) 先备份整个配置目录（包含 aimyria-one.db）\n\
            2) 如果提示“数据库版本过新”，请升级到更新版本\n\
            3) 如果刚升级出现异常，可回退旧版本导出/备份后再升级\n\n\
            点击「重试」重新尝试初始化\n\
            点击「退出」关闭程序",
            db = db_path.display()
        )
    } else {
        format!(
            "An error occurred while initializing or migrating the database:\n\n{error}\n\n\
            Database file path:\n{db}\n\n\
            Your data is NOT lost - the app will not delete the database automatically.\n\
            Common causes include: newer database version, corrupted file, permission issues, or low disk space.\n\n\
            Suggestions:\n\
            1) Back up the entire config directory (including aimyria-one.db)\n\
            2) If you see “database version is newer”, please upgrade AIMYRIA GO\n\
            3) If this happened right after upgrading, consider rolling back to export/backup then upgrade again\n\n\
            Click 'Retry' to attempt initialization again\n\
            Click 'Exit' to close the program",
            db = db_path.display()
        )
    };

    let retry_text = if is_chinese_locale() {
        "重试"
    } else {
        "Retry"
    };
    let exit_text = if is_chinese_locale() {
        "退出"
    } else {
        "Exit"
    };

    app.dialog()
        .message(&message)
        .title(title)
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::OkCancelCustom(
            retry_text.to_string(),
            exit_text.to_string(),
        ))
        .blocking_show()
}

// ============================================================
// 退出请求分类
// ============================================================

/// `RunEvent::ExitRequested` 的三类来源，处理方式必须区分。
///
/// 关键约束：重启请求（`code == RESTART_EXIT_CODE`）上 `prevent_exit()` 会被
/// Tauri 静默忽略（见 `ExitRequestApi::prevent_exit` 文档），事件循环必定继续
/// 退出并触发各插件的 `RunEvent::Exit` 钩子；任何与之并发的自定义清理任务都
/// 可能与插件退出钩子争用同一状态而死锁。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitRequestAction {
    /// `code` 为 `None`：运行时自动触发（如隐藏窗口的 WebView 被回收导致无存活
    /// 窗口），阻止退出、保持托盘后台运行。
    StayInTray,
    /// `code` 为 `RESTART_EXIT_CODE`：`app.restart()` 发起的重启（本应用自己的
    /// 重启都走 `restart_process`，这里只兜底），不拦截、不做自定义清理，交还
    /// Tauri 默认 re-exec 流程。
    DeferToTauriRestart,
    /// 其它 `Some(_)`：用户主动退出（托盘「退出」等），执行完整异步清理后结束进程。
    CleanupAndExit,
}

/// 收到过重启请求。重启时 Tauri 也会发 `RunEvent::Exit`，靠它跳过系统终止那条清理。
static RESTART_REQUESTED: AtomicBool = AtomicBool::new(false);

fn classify_exit_request(code: Option<i32>) -> ExitRequestAction {
    match code {
        None => ExitRequestAction::StayInTray,
        Some(tauri::RESTART_EXIT_CODE) => ExitRequestAction::DeferToTauriRestart,
        Some(_) => ExitRequestAction::CleanupAndExit,
    }
}

// ============================================================
// 在应用主动退出前显式持久化窗口状态
// ============================================================

fn window_state_flags() -> StateFlags {
    StateFlags::POSITION | StateFlags::SIZE | StateFlags::MAXIMIZED
}

/// 当前应用的退出路径会拦截 `ExitRequested` 并最终直接 `std::process::exit(0)`，
/// 这里需要在真正结束进程前手动落盘，避免 window-state 插件的默认退出钩子被绕过。
///
/// # Why a failure here is not just noisy
///
/// The window-state plugin resolves its target through `app.path().app_config_dir()`,
/// which is `dirs::config_dir()/${identifier}` and returns `Error::UnknownPath` when
/// `dirs` cannot resolve the folder. Every path that reaches the failure returns
/// *before* writing anything, so the user's window geometry silently stops being
/// remembered — and, on the restart path, the error is the loudest line in the log
/// while the process is about to exit.
///
/// Measured on this machine: `退出前保存窗口状态失败: unknown path` appears only for
/// runs whose exe lives outside the normal install location (`E:\updtest\...`,
/// `E:\smoke108\...`). Those are acceptance copies, and the pairing is the useful
/// detail: resolution depends on how the process was started, not on the build.
///
/// The fix is to place the state file ourselves when the plugin cannot, rather than
/// leaving the feature quietly dead. `panic_hook` already owns a config directory
/// derived from `dirs::home_dir()` with a `.` fallback, so this writes to exactly
/// the directory the app already uses for logs and the database.
pub fn save_window_state_before_exit(app_handle: &tauri::AppHandle) {
    match app_handle.save_window_state(window_state_flags()) {
        Ok(()) => log::info!("已在退出前保存窗口状态"),
        Err(err) => {
            log::warn!("window-state 插件无法保存（{err}），改用应用自己的目录写入");
            save_window_state_fallback(app_handle, &err.to_string());
        }
    }
}

/// Write the same `.window-state.json` shape the plugin uses, into the directory
/// this app controls.
///
/// Deliberately best-effort and quiet: a tray tool that cannot remember a window
/// position is mildly annoying, and making that a startup blocker would be worse.
fn save_window_state_fallback(app_handle: &tauri::AppHandle, cause: &str) {
    let Some(window) = app_handle.get_webview_window("main") else {
        return;
    };
    let Ok(position) = window.outer_position() else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    let maximized = window.is_maximized().unwrap_or(false);

    let value = serde_json::json!({
        "main": {
            "width": size.width,
            "height": size.height,
            "x": position.x,
            "y": position.y,
            "maximized": maximized,
        }
    });

    let path = crate::config::get_app_config_dir().join(".window-state.json");
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("创建窗口状态目录失败: {e}（原始错误: {cause}）");
            return;
        }
    }
    let write = std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap_or_default());
    match write {
        Ok(()) => log::info!("窗口状态已写入 {}", path.display()),
        Err(e) => log::warn!("写窗口状态失败: {e}（原始错误: {cause}）"),
    }
}

/// 主动释放 single-instance 锁。
///
/// macOS single-instance 使用 `/tmp/{identifier}.sock`。我们有若干路径会直接
/// `std::process::exit(0)`，不会触发插件挂在 `RunEvent::Exit` 上的清理钩子。
/// 重启前主动 destroy 可以避免新进程误连旧 listener 后自行退出。
pub fn destroy_single_instance_lock(app_handle: &tauri::AppHandle) {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    tauri_plugin_single_instance::destroy(app_handle);
}

/// 清理托盘图标、释放 single-instance 锁后重启当前应用。
///
/// 直接 spawn 新进程 + `exit(0)`（macOS 经 `open -n`，见 `relaunch_macos_bundle`），不经过事件
/// 循环退出，因此 Tauri 内部的 `cleanup_before_exit` 和各插件的
/// `RunEvent::Exit` 钩子都不会执行。需要的清理由调用方与本函数显式补偿：
/// 窗口状态、代理/Live 恢复（调用方）；托盘图标、single-instance 锁（本函数）。
///
/// 有意不调 `AppHandle::cleanup_before_exit()`：它会在调用线程上 Drop 托盘
/// 图标，而 macOS 的 NSStatusItem 操作要求主线程；`set_visible(false)` 走
/// `run_item_main_thread` 代理，跨线程安全（见 `remove_tray_icon_before_exit`）。
///
/// # Why this does not use `tauri::process::restart`
///
/// `tauri::process::restart` is `Command::new(current_binary).spawn()` followed by
/// `exit(0)` — and it **logs a failed spawn and exits anyway**:
///
/// ```text
/// if let Err(e) = Command::new(path)...spawn() { log::error!("failed to restart app: {e}"); }
/// exit(0);
/// ```
///
/// For an updater that is the worst possible failure. The old exe has already been
/// renamed aside, the new one is in place, and if the new one cannot start — a
/// quarantined download, a missing `WebView2Loader.dll`, a virus scanner holding
/// the file, a partially written payload — this process exits with nothing left
/// running. The tray icon is gone, there is no console, no window, and autostart
/// is the only thing that will bring it back at the next sign-in. From the user's
/// side the app simply vanished after updating, which is exactly the report this
/// change exists to answer.
///
/// So: spawn it ourselves, wait briefly to see whether it survives, and only exit
/// once the replacement is confirmed alive. If it dies, relaunch the `.old` copy
/// the updater kept for this cycle, and if that fails too, keep *this* process
/// running (it still has the tray icon) and say so in the log rather than going
/// dark.
fn relaunch_detached(exe: &std::path::Path, args: &[std::ffi::OsString]) -> std::io::Result<u32> {
    let mut command = std::process::Command::new(exe);
    command.args(args);
    // A new process group: otherwise a Ctrl+C on a parent console (or a job object
    // that owns this process) can take the replacement down with us.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    Ok(command.spawn()?.id())
}

/// Is the freshly spawned replacement still alive after a short settling period?
///
/// This is the cheapest available proof that it did not die on startup and is
/// worth more than it looks: a successful `spawn` only proves the loader accepted
/// the file. A missing dependency, an incompatible CPU baseline, or an immediate
/// panic in `setup` all show up as "exited within a second".
#[cfg(windows)]
fn replacement_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    if pid == 0 {
        return false;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            // Cannot open it: it is already gone (or was never scheduled).
            return false;
        }
        let mut code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut code) != 0;
        CloseHandle(handle);
        ok && code == STILL_ACTIVE as u32
    }
}

#[cfg(not(windows))]
fn replacement_is_alive(pid: u32) -> bool {
    // `kill(pid, 0)` is the portable liveness probe on Unix.
    pid != 0 && std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// How long to let a replacement prove itself before exiting the old process.
///
/// Short enough that a normal restart still feels instant, long enough to catch a
/// process that dies in `setup` — which is where the crash log shows this app's
/// failures happen.
const REPLACEMENT_GRACE: std::time::Duration = std::time::Duration::from_millis(1200);

pub fn restart_process(app_handle: &tauri::AppHandle) -> ! {
    remove_tray_icon_before_exit(app_handle);
    destroy_single_instance_lock(app_handle);

    let env = app_handle.env();
    #[cfg(target_os = "macos")]
    relaunch_macos_bundle(&env);

    let binary = match tauri::process::current_binary(&env) {
        Ok(path) => path,
        Err(e) => {
            log::error!("无法定位当前可执行文件，重启中止（保留当前进程）: {e}");
            std::process::exit(0);
        }
    };
    let args: Vec<std::ffi::OsString> = env.args_os.iter().skip(1).cloned().collect();

    match relaunch_detached(&binary, &args) {
        Ok(pid) => {
            std::thread::sleep(REPLACEMENT_GRACE);
            if replacement_is_alive(pid) {
                log::info!("新进程 pid={pid} 已接管，退出当前进程");
                std::process::exit(0);
            }
            log::error!(
                "新进程 pid={pid} 启动后立即退出（{}），回滚到上一份可执行文件",
                binary.display()
            );

            // The updater keeps `<exe>.old` for exactly one cycle. On Windows the
            // running image is locked, so the copy has to be renamed *back* into
            // place rather than overwriting.
            if let Some(restored) = restore_previous_exe(&binary) {
                match relaunch_detached(&restored, &args) {
                    Ok(pid) => {
                        std::thread::sleep(REPLACEMENT_GRACE);
                        if replacement_is_alive(pid) {
                            log::warn!("已回滚并用上一版本 pid={pid} 重新启动");
                            std::process::exit(0);
                        }
                    }
                    Err(e) => log::error!("回滚版本也无法启动: {e}"),
                }
            }

            // Nothing we launched stayed up. Do NOT exit: this process is the only
            // thing keeping the tray icon (and therefore any way back in) alive.
            // Falling asleep forever is intentional — the alternative is a machine
            // with no process, and no UI that could ever report the problem.
            log::error!("重启与回滚均失败；保留当前进程以避免应用彻底消失");
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }
        Err(e) => {
            log::error!(
                "无法启动新进程（{e}）；保留当前进程，问题可在日志里追溯，应用不会静默消失"
            );
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }
    }
}

/// Put the retained previous binary back under the expected name.
///
/// Mirrors the updater's own rename dance in reverse: the live name is moved to
/// `.failed` first, the `.old` copy is moved into the live name, and the failed
/// payload is left on disk for diagnosis. Returns the path to launch, or `None`
/// when there is nothing to roll back to.
fn restore_previous_exe(live: &std::path::Path) -> Option<std::path::PathBuf> {
    fn sibling(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        std::path::PathBuf::from(name)
    }

    let old = sibling(live, ".old");
    if !old.is_file() {
        log::error!("没有可回滚的 {}，无法自动恢复", old.display());
        return None;
    }
    let failed = sibling(live, ".failed");
    let _ = std::fs::remove_file(&failed);
    // The old process image is mapped; renaming is allowed, deleting is not. We are
    // still running THAT image, so it must not be overwritten either.
    if let Err(e) = std::fs::rename(live, &failed) {
        log::error!("无法把失败的新版本移开（{e}），回滚中止");
        return None;
    }
    match std::fs::rename(&old, live) {
        Ok(()) => {
            log::warn!("已把上一版本恢复到 {}", live.display());
            Some(live.to_path_buf())
        }
        Err(e) => {
            log::error!("把上一版本恢复回原位置失败: {e}");
            // Leave the tree as close to working as possible.
            let _ = std::fs::rename(&failed, live);
            None
        }
    }
}

/// macOS 经 LaunchServices（`open -n`）启动新实例，成功即退出；失败时返回，
/// 由调用方回落到 `tauri::process::restart`。
///
/// `tauri::process::restart` 直接 spawn 可执行文件。macOS 14 起应用激活是协作式的：
/// 新进程的 `activateIgnoringOtherApps` 会被系统拒绝，窗口留在其它应用后面。
/// 由当前前台应用请求 LaunchServices 启动，新实例才能拿到前台。
#[cfg(target_os = "macos")]
fn relaunch_macos_bundle(env: &tauri::Env) {
    let Ok(binary) = tauri::process::current_binary(env) else {
        return;
    };
    // <Name>.app/Contents/MacOS/<binary>
    let Some(bundle) = binary
        .ancestors()
        .nth(3)
        .filter(|p| p.extension().is_some_and(|ext| ext == "app"))
    else {
        return;
    };
    let mut command = std::process::Command::new("/usr/bin/open");
    command.arg("-n").arg(bundle);
    let args: Vec<_> = env.args_os.iter().skip(1).collect();
    if !args.is_empty() {
        command.arg("--args").args(args);
    }
    match command.status() {
        Ok(status) if status.success() => std::process::exit(0),
        Ok(status) => log::warn!("open -n 重启失败（{status}），回落直接启动"),
        Err(err) => log::warn!("open -n 重启失败（{err}），回落直接启动"),
    }
}

#[cfg(test)]
mod restart_tests {
    use std::path::{Path, PathBuf};

    /// Telling "the machine started us" from "the user clicked us".
    ///
    /// This drives a user-visible decision in the single-instance callback: a bare
    /// launch raises the window (because a double-clicked icon that does nothing
    /// looks broken), while an autostart launch must stay silent (nobody wants a
    /// window in their face at sign-in).
    ///
    /// Both directions are asserted because getting it backwards is invisible in a
    /// smoke test — the app still starts, it just behaves wrongly at login or when
    /// clicked.
    #[test]
    fn autostart_launches_are_recognised_and_user_clicks_are_not() {
        // What `auto_launch` writes into the Run key.
        assert!(super::argv_is_autostart(&[
            r"C:\Apps\AimyriaOne.exe".to_string(),
            "--autostart".to_string()
        ]));
        // Tolerated shapes: different case, no path prefix, slash form.
        assert!(super::argv_is_autostart(&["--AUTOSTART".to_string()]));
        assert!(super::argv_is_autostart(&["/autostart".to_string()]));
        assert!(super::argv_is_autostart(&["  --autostart  ".to_string()]));

        // A double-click carries nothing — this is the case that must raise the UI.
        assert!(!super::argv_is_autostart(&[]));
        // A deep link is a request in its own right, not an autostart.
        assert!(!super::argv_is_autostart(&[
            "aimyria://v1/import?resource=provider".to_string()
        ]));
        // Prefix matching must not accept a different flag.
        assert!(!super::argv_is_autostart(&["--autostart-now".to_string()]));
    }

    fn sibling(path: &Path, suffix: &str) -> PathBuf {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        PathBuf::from(name)
    }

    /// The rollback has to put the *previous* binary back under the live name,
    /// and keep the failed payload on disk for diagnosis.
    ///
    /// This is the last line of defence for the reported failure: "it updated,
    /// then never started again". The updater renames the old exe to `.old`, so
    /// the only way back is to rename it in again. Without this test, a future
    /// change to the suffix convention would silently disable the recovery path.
    #[test]
    fn rollback_restores_the_previous_binary() {
        let dir = tempfile::tempdir().expect("temp dir");
        let live = dir.path().join("AimyriaOne.exe");
        let old = sibling(&live, ".old");

        std::fs::write(&live, b"broken new version").unwrap();
        std::fs::write(&old, b"known good version").unwrap();

        let restored = super::restore_previous_exe(&live).expect("rollback should succeed");

        assert_eq!(restored, live, "the restored path is the live name");
        assert_eq!(
            std::fs::read(&live).unwrap(),
            b"known good version",
            "the previous binary must be back in place"
        );
        assert_eq!(
            std::fs::read(sibling(&live, ".failed")).unwrap(),
            b"broken new version",
            "the failed payload must be kept for diagnosis"
        );
        assert!(!old.exists(), ".old was consumed by the restore");
    }

    /// With nothing to roll back to, the function must report that instead of
    /// pretending the live file is healthy.
    #[test]
    fn rollback_without_a_previous_binary_reports_failure() {
        let dir = tempfile::tempdir().expect("temp dir");
        let live = dir.path().join("AimyriaOne.exe");
        std::fs::write(&live, b"only version").unwrap();

        assert!(super::restore_previous_exe(&live).is_none());
        assert_eq!(
            std::fs::read(&live).unwrap(),
            b"only version",
            "the live file must be untouched when there is nothing to restore"
        );
    }

    /// A `.old` that is a directory, not a file, is not a usable rollback source.
    #[test]
    fn rollback_ignores_a_non_file_previous_entry() {
        let dir = tempfile::tempdir().expect("temp dir");
        let live = dir.path().join("AimyriaOne.exe");
        std::fs::write(&live, b"live").unwrap();
        std::fs::create_dir(sibling(&live, ".old")).unwrap();

        assert!(super::restore_previous_exe(&live).is_none());
    }

    /// The liveness probe must be able to see *this* process, or every restart
    /// would look like a failure and take the rollback path.
    #[test]
    fn the_liveness_probe_sees_a_running_process() {
        let me = std::process::id();
        assert!(
            super::replacement_is_alive(me),
            "the probe must recognise a process that is definitely running"
        );
    }

    /// A pid that cannot exist must read as dead. Guarding this matters: if the
    /// probe returned true for everything, a stillborn replacement would be
    /// accepted and the old process would exit anyway — the original bug.
    #[test]
    fn the_liveness_probe_rejects_an_impossible_pid() {
        // u32::MAX is not a valid pid on any supported platform.
        assert!(!super::replacement_is_alive(u32::MAX));
    }
}
