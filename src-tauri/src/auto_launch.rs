use crate::error::AppError;
use auto_launch::{AutoLaunch, AutoLaunchBuilder};

/// Name of the `Run` value this app owns.
///
/// One definition, because the enable/disable/status paths must agree on it: a
/// value written under one name and checked under another is a setting that
/// silently does nothing.
const APP_NAME: &str = "AimyriA go";

/// 获取 macOS 上的 .app bundle 路径
/// 将 `/path/to/CC Switch.app/Contents/MacOS/CC Switch` 转换为 `/path/to/CC Switch.app`
#[cfg(target_os = "macos")]
fn get_macos_app_bundle_path(exe_path: &std::path::Path) -> Option<std::path::PathBuf> {
    let path_str = exe_path.to_string_lossy();
    // 查找 .app/Contents/MacOS/ 模式
    if let Some(app_pos) = path_str.find(".app/Contents/MacOS/") {
        let app_bundle_end = app_pos + 4; // ".app" 的结束位置
        Some(std::path::PathBuf::from(&path_str[..app_bundle_end]))
    } else {
        None
    }
}

/// 初始化 AutoLaunch 实例
fn get_auto_launch() -> Result<AutoLaunch, AppError> {
    let exe_path =
        std::env::current_exe().map_err(|e| AppError::Message(format!("无法获取应用路径: {e}")))?;

    // macOS 需要使用 .app bundle 路径，否则 AppleScript login item 会打开终端
    #[cfg(target_os = "macos")]
    let app_path = get_macos_app_bundle_path(&exe_path).unwrap_or(exe_path);

    #[cfg(not(target_os = "macos"))]
    let app_path = exe_path;

    // 使用 AutoLaunchBuilder 消除平台差异
    // macOS: 使用 AppleScript 方式（默认），需要 .app bundle 路径
    // Windows/Linux: 使用注册表/XDG autostart
    let auto_launch = AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&app_path.to_string_lossy())
        .build()
        .map_err(|e| AppError::Message(format!("创建 AutoLaunch 失败: {e}")))?;

    Ok(auto_launch)
}

/// 启用开机自启
pub fn enable_auto_launch() -> Result<(), AppError> {
    let auto_launch = get_auto_launch()?;
    auto_launch
        .enable()
        .map_err(|e| AppError::Message(format!("启用开机自启失败: {e}")))?;
    log::info!("已启用开机自启");
    Ok(())
}

/// 禁用开机自启
pub fn disable_auto_launch() -> Result<(), AppError> {
    let auto_launch = get_auto_launch()?;
    auto_launch
        .disable()
        .map_err(|e| AppError::Message(format!("禁用开机自启失败: {e}")))?;
    log::info!("已禁用开机自启");
    Ok(())
}

/// 检查是否已启用开机自启
///
/// # Why this does not just ask the `auto-launch` crate
///
/// `AutoLaunch::is_enabled()` on Windows answers a weaker question than the one
/// that matters: it reports whether a value named `AimyriA go` exists under
/// `Run`, and never compares the path inside it.
///
/// That makes a stale entry look like a healthy one. Every ordinary way a machine
/// reaches that state — a previous install to a different directory, an uninstall
/// that removed the binary but left the registry value, the user moving the
/// folder — leaves a value pointing at an exe that no longer exists.
/// `is_enabled()` then answers true, `first_run::seed_autostart` returns early,
/// and the path is never corrected. The tool looks installed, works when launched
/// by hand, and never starts at login. Since a deep link is the only way an
/// account gets bound, an app that is not running is an app that quietly does
/// nothing.
///
/// Reporting "not enabled" for a mismatched path is exactly what makes the
/// self-healing in `first_run` self-heal: the caller then calls
/// [`enable_auto_launch`], which rewrites the value with the current path.
///
/// # Why this does not consult the Task Manager override
///
/// The crate also folds in `StartupApproved\Run`, and treats a non-zero tail as
/// "disabled" — which is why it can report false and then have the caller
/// re-enable. But that key records a *user's* decision to switch the entry off in
/// Task Manager. Re-enabling it from here would overwrite that decision on every
/// launch, which is the same class of bug as overwriting a chosen setting. The
/// user's opt-out is already recorded explicitly in `autostart_user_opted_out`
/// and consulted before this function is reached; a second, invisible opt-out
/// that we refuse to honour is a bug, not a safety net.
#[cfg(target_os = "windows")]
pub fn is_auto_launch_enabled() -> Result<bool, AppError> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = match hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run") {
        Ok(key) => key,
        // No `Run` key at all means nothing can be starting at login.
        Err(_) => return Ok(false),
    };

    let registered: String = match run.get_value(APP_NAME) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };

    let exe = std::env::current_exe()
        .map_err(|e| AppError::Message(format!("无法获取应用路径: {e}")))?
        .display()
        .to_string();

    Ok(normalize_run_value(&registered) == normalize_run_value(&exe))
}

/// Strip the quotes Windows wraps around a command path, leaving a comparable form.
#[cfg(target_os = "windows")]
fn normalize_run_value(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim()
        // Paths are compared case-insensitively: Windows treats them that way, and
        // a rebuild differing only by case must not read as a stale entry. That
        // would rewrite the value on every launch.
        .to_lowercase()
}

/// 检查是否已启用开机自启
#[cfg(not(target_os = "windows"))]
pub fn is_auto_launch_enabled() -> Result<bool, AppError> {
    let auto_launch = get_auto_launch()?;
    auto_launch
        .is_enabled()
        .map_err(|e| AppError::Message(format!("检查开机自启状态失败: {e}")))
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    /// A value written quoted must compare equal to the bare path, or every launch
    /// would decide the entry was stale and rewrite it.
    #[test]
    fn quoting_and_case_do_not_make_a_healthy_entry_look_stale() {
        assert_eq!(
            normalize_run_value(r#""C:\Apps\Aimyria\AimyriaOne.exe""#),
            normalize_run_value(r"C:\Apps\Aimyria\AimyriaOne.exe"),
        );
        assert_eq!(
            normalize_run_value(r"C:\APPS\Aimyria\AimyriaOne.exe"),
            normalize_run_value(r"c:\apps\aimyria\aimyriaone.exe"),
        );
    }

    /// A path that differs is a different entry. This is the stale case the whole
    /// function exists to catch: without it, `seed_autostart` returns early and the
    /// machine keeps a `Run` value pointing at a binary that is gone.
    #[test]
    fn a_value_pointing_somewhere_else_is_reported_as_stale() {
        assert_ne!(
            normalize_run_value(r"C:\Users\someone\AppData\Local\AimyriA go\aimyria-one.exe"),
            normalize_run_value(r"C:\Apps\Aimyria\AimyriaOne.exe"),
        );
    }
}
