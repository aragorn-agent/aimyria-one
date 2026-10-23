use crate::error::AppError;
use auto_launch::{AutoLaunch, AutoLaunchBuilder};

/// Name of the `Run` value this app owns.
///
/// One definition, because the enable/disable/status paths must agree on it: a
/// value written under one name and checked under another is a setting that
/// silently does nothing.
///
/// This is the product name, which means renaming the product renames this
/// value. See [`LEGACY_APP_NAMES`] for why that is not free.
const APP_NAME: &str = "AIMYRIA GO";

/// Run values written by an *actually different* earlier product name.
///
/// Only `Aimyria One` belongs here. `AimyriA go` must NOT be added: registry
/// value names are **case-insensitive**, so `AimyriA go` and `AIMYRIA GO` are the
/// same value. Listing it as "legacy" made the enable path write the current
/// value and then immediately delete it (measured: count 1 -> 0), which silently
/// turned autostart off for every machine that had ever run an older build.
///
/// The remaining case difference is handled by [`migrate_legacy_autostart`],
/// which recreates the value under the current spelling instead of deleting it.
const LEGACY_APP_NAMES: [&str; 1] = ["Aimyria One"];

/// The spelling earlier builds used for the *same* Run value as [`APP_NAME`].
///
/// Same registry value, different case — see [`LEGACY_APP_NAMES`] for why it
/// cannot be deleted.
const LEGACY_CASING: &str = "AimyriA go";

/// Delete `Run` values left by earlier product names.
///
/// Best-effort on purpose: a machine where the registry is locked by policy
/// still gets a working autostart from the current name, and the failure is
/// logged rather than turned into an error the user cannot act on.
#[cfg(target_os = "windows")]
pub fn remove_legacy_run_values() {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(run) = hkcu.open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        winreg::enums::KEY_READ | winreg::enums::KEY_WRITE,
    ) else {
        return;
    };

    for name in LEGACY_APP_NAMES {
        // 防御：万一有人把当前名的某个大小写变体写进这张表，这里必须拒绝删除 ——
        // 注册表值名大小写不敏感，删"变体"就是删当前值本身。
        if name.eq_ignore_ascii_case(APP_NAME) {
            continue;
        }
        if run.get_value::<String, _>(name).is_ok() {
            match run.delete_value(name) {
                Ok(()) => log::info!("已移除旧产品名留下的自启项: {name}"),
                Err(e) => log::warn!("移除旧自启项 {name} 失败: {e}"),
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn remove_legacy_run_values() {}

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
    // 上面这个 crate 写的是**无引号**路径（实测还带一个尾随空格）。路径里有空格时
    // Windows 会把第一个空格前当命令、后面当参数，自启直接失效 —— 而
    // `C:\Users\John Smith\...` 这种用户名在 Windows 上极常见。
    // 所以再覆盖一次带引号的写法；内容一致，只是 quoting 不同。
    rewrite_run_value_quoted();
    // Only after the current value exists: if `enable` failed there is nothing
    // to leave behind, and removing the legacy value first would trade a
    // duplicate for no autostart at all.
    remove_legacy_run_values();
    log::info!("已启用开机自启");
    Ok(())
}

/// The argument the start-on-login entry passes.
///
/// This is load-bearing, not decoration. The single-instance callback uses it to
/// tell "the machine started us" from "the user double-clicked the icon": absent
/// the flag, a login start would be treated as a user request and pop the window
/// in someone's face every time they sign in.
///
/// It also keeps older behaviour intact — before it existed, a bare relaunch was
/// ignored outright, so the login path showing no window was the *implicit*
/// contract. Now it is explicit.
const AUTOSTART_FLAG: &str = "--autostart";

/// Rewrite the `Run` value with the path quoted.
///
/// Best-effort: when this fails the unquoted value the crate wrote is still
/// there, which works for space-free paths — the majority — so this is not an
/// error the user needs to see.
#[cfg(target_os = "windows")]
fn rewrite_run_value_quoted() {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let quoted = format!("\"{}\" {}", exe.display(), AUTOSTART_FLAG);

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(run) = hkcu.open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        winreg::enums::KEY_READ | winreg::enums::KEY_WRITE,
    ) else {
        return;
    };
    if let Err(e) = run.set_value(APP_NAME, &quoted) {
        log::warn!("自启项写入带引号路径失败（无空格的路径仍可用）: {e}");
    }
}

#[cfg(not(target_os = "windows"))]
fn rewrite_run_value_quoted() {}

/// 禁用开机自启
pub fn disable_auto_launch() -> Result<(), AppError> {
    let auto_launch = get_auto_launch()?;
    auto_launch
        .disable()
        .map_err(|e| AppError::Message(format!("禁用开机自启失败: {e}")))?;
    // 用户关闭自启时也要清旧名，否则"关了还在启动"。
    remove_legacy_run_values();
    log::info!("已禁用开机自启");
    Ok(())
}

/// 检查是否已启用开机自启
///
/// # Why this does not just ask the `auto-launch` crate
///
/// `AutoLaunch::is_enabled()` on Windows answers a weaker question than the one
/// that matters: it reports whether a value named `AIMYRIA GO` exists under
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

    let exe = std::env::current_exe()
        .map_err(|e| AppError::Message(format!("无法获取应用路径: {e}")))?
        .display()
        .to_string();
    let want = normalize_run_value(&exe);

    // 只查当前名。旧拼写 `AimyriA go` 与它大小写不同、但**是同一个注册表值**，
    // 所以这里不需要也不能额外识别"旧名"——加了反而会把一个值当成两个。
    if let Ok(registered) = run.get_value::<String, _>(APP_NAME) {
        return Ok(normalize_run_value(&registered) == want);
    }

    // 真正不同的旧产品名（`Aimyria One`）仍算"已启用"，避免升级后重复添加。
    for name in LEGACY_APP_NAMES {
        if let Ok(registered) = run.get_value::<String, _>(name) {
            if normalize_run_value(&registered) == want {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

/// Strip the quotes Windows wraps around a command path, and drop the
/// start-on-login flag, leaving a comparable exe path.
///
/// The flag MUST be removed before comparing: `is_auto_launch_enabled` compares
/// this form against a bare `current_exe()`, so leaving `"C:\...\x.exe" --autostart`
/// intact would make a perfectly healthy entry read as "points somewhere else" and
/// be rewritten on every single launch — a harmless-looking write that repaints the
/// value and, worse, makes `seed_autostart` re-enable an entry the user had turned
/// off in Task Manager.
///
/// `--autostart` is written by [`rewrite_run_value_quoted`]; older installs have a
/// bare path, which still normalises to the same string.
#[cfg(target_os = "windows")]
fn normalize_run_value(value: &str) -> String {
    let trimmed = value.trim();
    // Prefer the quoted exe path when present: the tail is our own argument.
    let path_part = match trimmed.strip_prefix('"') {
        Some(rest) => match rest.split_once('"') {
            Some((path, _args)) => path,
            // Unterminated quote: fall back to the whole string, trimmed below.
            None => trimmed.trim_matches('"'),
        },
        None => {
            // Unquoted: a bare path may itself contain spaces, so strip the known
            // flag rather than splitting on whitespace.
            trimmed
                .strip_suffix(AUTOSTART_FLAG)
                .unwrap_or(trimmed)
                .trim()
        }
    };
    path_part
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

/// Align the autostart entry with the current product name.
///
/// Two different situations, and they need different handling:
///
/// 1. **Same value, different spelling** (`AimyriA go` vs `AIMYRIA GO`). Registry
///    value names are case-insensitive, so this is one value that Windows keeps
///    displaying under the casing it was first written with. Rewriting the data
///    does not change the name; only delete-then-recreate does. That is what this
///    does — and the recreate is unconditional right after the delete, so a
///    machine is never left without autostart.
///
/// 2. **Genuinely different old names** (`Aimyria One`). Those are separate
///    values; [`remove_legacy_run_values`] removes them.
///
/// In both cases the value must already point at *this* exe: it is the same
/// startup, just renamed. Anything else is left alone for the caller to fix
/// through the normal enable path.
#[cfg(target_os = "windows")]
pub fn migrate_legacy_autostart() {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let exe = exe.display().to_string();
    let want = normalize_run_value(&exe);

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(run) = hkcu.open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        winreg::enums::KEY_READ | winreg::enums::KEY_WRITE,
    ) else {
        return;
    };

    let current = run
        .get_value::<String, _>(APP_NAME)
        .ok()
        .filter(|v| normalize_run_value(v) == want);

    if current.is_none() {
        // 没有当前名的值：可能只有旧名，也可能什么都没开。交给 enable 路径判断。
        let has_legacy_for_us = LEGACY_APP_NAMES.iter().chain(std::iter::once(&LEGACY_CASING))
            .any(|name| {
                run.get_value::<String, _>(name)
                    .map(|v| normalize_run_value(&v) == want)
                    .unwrap_or(false)
            });
        if has_legacy_for_us {
            match enable_auto_launch() {
                Ok(()) => log::info!("开机自启已迁到当前产品名"),
                Err(e) => log::warn!("迁移开机自启失败（旧项保留，仍可启动）: {e}"),
            }
        }
        return;
    }

    // 当前名已存在且指向本程序。只剩"显示的大小写"可能还是旧拼写。
    // 判断方法：注册表返回值名时，同一路径下 Windows 保留首次写入的大小写。
    remove_legacy_run_values();
    if !run_is_spelled_currently(&exe) {
        // 重建一次，让 Task Manager → 启动 里显示的是 AIMYRIA GO。
        // 顺序是 delete 之后立刻 set，中间没有别的分支，失败也会被 set 覆盖回来。
        let _ = run.delete_value(APP_NAME);
        match enable_auto_launch() {
            Ok(()) => log::info!("开机自启显示名已更新为当前产品名"),
            Err(e) => log::warn!("更新自启显示名失败: {e}"),
        }
    }
}

/// Whether the `Run` value is stored under the current exact spelling.
///
/// `winreg` reads values case-insensitively, so the stored spelling has to come
/// from enumerating the names. A machine that never ran the old build already
/// matches and is left untouched.
#[cfg(target_os = "windows")]
fn run_is_spelled_currently(_exe: &str) -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(run) = hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run") else {
        return true;
    };
    for name in run.enum_values().flatten().map(|(name, _)| name) {
        if name.eq_ignore_ascii_case(APP_NAME) {
            return name == APP_NAME;
        }
    }
    // 没找到（不该发生）：不声称需要重建，避免无谓的删写。
    true
}

#[cfg(not(target_os = "windows"))]
pub fn migrate_legacy_autostart() {}

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

    /// The start-on-login flag must not affect the comparison.
    ///
    /// `is_auto_launch_enabled` compares this normalised form against a bare
    /// `current_exe()`. If `--autostart` survived normalisation, every entry this
    /// build writes would read as pointing elsewhere, `seed_autostart` would rewrite
    /// it on every launch, and a user's Task Manager opt-out would be undone once
    /// per start.
    #[test]
    fn the_autostart_flag_is_not_part_of_the_compared_path() {
        let bare = normalize_run_value(r"C:\Apps\Aimyria\AimyriaOne.exe");
        let flagged = normalize_run_value(r#""C:\Apps\Aimyria\AimyriaOne.exe" --autostart"#);
        assert_eq!(
            bare, flagged,
            "a flagged entry must compare equal to the same exe without the flag"
        );

        // An unquoted legacy shape (the auto-launch crate's own output) also has to
        // classify, because older installs still carry it until the first enable.
        let unquoted = normalize_run_value(r"C:\Apps\Aimyria\AimyriaOne.exe --autostart");
        assert_eq!(bare, unquoted);

        // A genuinely different path must still be detected as stale.
        assert_ne!(
            flagged,
            normalize_run_value(r"C:\Elsewhere\AimyriaOne.exe"),
            "different paths must not collapse to the same value"
        );
    }

    /// `Run` value names are case-insensitive on Windows, so a "legacy" name that
    /// differs from the current one only by case is the SAME value.
    ///
    /// This was a real regression: listing `AimyriA go` as legacy made the enable
    /// path write the current value and then delete it (measured 1 -> 0), silently
    /// turning autostart off for every machine that had run an older build. The
    /// guard is asserted here because the failure is invisible in unit tests that
    /// only look at the paths.
    #[test]
    fn a_legacy_name_that_is_only_a_case_variant_is_never_deleted() {
        for name in LEGACY_APP_NAMES {
            assert!(
                !name.eq_ignore_ascii_case(APP_NAME),
                "{name} is the same registry value as {APP_NAME}; deleting it would                  remove the autostart entry we just wrote",
            );
        }
        // The spelling variant we DO keep exists only to be recreated, never deleted.
        assert!(LEGACY_CASING.eq_ignore_ascii_case(APP_NAME));
        assert_ne!(LEGACY_CASING, APP_NAME, "it must differ for a rewrite to do anything");
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
