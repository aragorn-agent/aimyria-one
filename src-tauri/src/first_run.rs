// Launch bootstrap: make the machine match what this tool needs to work.
//
// # Both jobs run on every launch
//
// Protocol registration and start-on-login are statements about the
// *installation*, and both are cheap, idempotent and self-healing. Neither is
// gated on a "have I done this already" marker.
//
// An earlier version had one marker covering both, and every routine way of
// invalidating the machine's state — an uninstall that removes registry values
// while deliberately keeping the user data, a reinstall to a different
// directory, a user who cleaned HKCU by hand — left the tool convinced it was
// set up when it was not. Uninstall keeping `~/.aimyria` is deliberate: it
// holds the API key, and the reason people uninstall is usually "not right
// now", not "I don't want this account". But the bootstrap marker lives in that
// same directory, so preserving it silently disabled the setup the reinstall was
// supposed to restore.
//
// The consequence was that the tool sat in the tray looking installed while a
// reinstalled copy could not receive a single deep link — and deep links are
// how an account gets bound. Nothing about that is acceptable for the product's
// central promise.
//
// # The one thing that does gate anything
//
// [`AUTOSTART_SKIPPED`] — the single record that the user turned start-on-login
// off. It is consulted on every launch, so that decision is permanent and is
// never overridden.
//
// # Why autostart defaults to on
//
// The alternative is a tool that has to be launched by hand before a deep link
// can work — exactly the extra step the product exists to remove.
//
// It is still a switch: the user can turn it off in Settings, and we never
// re-enable it afterwards.
//
// This is a judgment call about someone else's machine, so it is stated plainly
// rather than buried: we add one registry value under
// `HKCU\...\CurrentVersion\Run`, pointing at our own exe. No service, no
// scheduled task, no system-wide change, no administrator rights.

use tauri::{AppHandle, Manager};

use crate::store::AppState;

/// Set once the user turns autostart off. This flag — and only this flag — is
/// what keeps autostart off. Nothing else may, because nothing else records the
/// user's decision.
const AUTOSTART_SKIPPED: &str = "autostart_user_opted_out";

/// Run the launch bootstrap. Safe and cheap to call on every launch.
///
/// Both things here are statements about the installation, not one-time events,
/// so neither is gated on a "have I done this already" marker. An earlier
/// version had one marker covering both, and every way of invalidating the
/// machine's state — an uninstall that removes registry values while leaving the
/// user data, a reinstall to a different directory, a user who cleaned HKCU —
/// left the tool believing it was set up when it was not.
///
/// That failure mode is bad here specifically: deep links are how an account
/// gets bound, so the tool would sit in the tray looking installed while doing
/// nothing at all.
pub fn maybe_run(app: &AppHandle) {
    if let Err(e) = register_protocol(app) {
        log::warn!("注册 aimyria:// 失败，深链将不可用: {e}");
    }

    reassert_autostart(app);
}

/// Bring start-on-login in line with what the user asked for, or with the
/// default if they never said.
///
/// Cheap in the common case: `seed_autostart` reads the current state first and
/// returns without writing when it already matches.
fn reassert_autostart(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    if let Err(e) = seed_autostart(&state.db) {
        log::warn!("设置开机自启失败（用户可在设置里手动开启）: {e}");
    }
}

/// Turn on start-on-login unless the user has already opted out.
///
/// The opt-out check is what makes the default defensible: it is consulted on
/// every launch, not only the first, so turning the switch off is permanent.
fn seed_autostart(db: &crate::database::Database) -> Result<(), crate::error::AppError> {
    if db.get_setting(AUTOSTART_SKIPPED)?.is_some() {
        log::info!("用户已关闭开机自启，不再自动开启");
        return Ok(());
    }

    if crate::auto_launch::is_auto_launch_enabled().unwrap_or(false) {
        return Ok(());
    }

    crate::auto_launch::enable_auto_launch()?;
    log::info!("已默认开启开机自启（可在设置中关闭）");
    Ok(())
}

/// The URL schemes this app must own.
///
/// Both are load-bearing, and for the same user journey:
///
/// - `aimyria://` — what the console's import button emits once the site is
///   updated, and what our own scripts open.
/// - `ccswitch://` — what the console's 「导入到CCS」 button *still* emits, and
///   what our scripts fall back to when anything goes wrong.
///
/// Registering only `aimyria://` looks sufficient and is not. On a machine where
/// an older CC-Switch build is present, `ccswitch://` still belongs to it, so a
/// click that falls back to the original link launches *their* binary — which, on
/// start, registers `aimyria://` to itself, taking our scheme back. The two then
/// trade ownership depending on which one ran last. On a machine with no such
/// build, the fallback reaches nothing at all: the browser shows its "no app
/// associated" dead end, which is the exact failure this product exists to remove.
///
/// The upstream plugin's `register_all()` would cover both, but it is gated to
/// Linux and debug builds, so on a shipped Windows release it never runs and the
/// schemes the config declares are, in practice, only registered by this
/// function.
const PROTOCOL_SCHEMES: [&str; 2] = ["aimyria", "ccswitch"];

/// Make sure the OS knows about both URL schemes this app answers to.
///
/// Idempotent: every value is overwritten in place. Windows-only by design —
/// macOS and Linux use per-user desktop files that the installer owns, and we do
/// not ship those platforms yet; writing a desktop file from the app would put
/// install behaviour in two places.
#[cfg(target_os = "windows")]
fn register_protocol(_app: &AppHandle) -> Result<(), crate::error::AppError> {
    use crate::error::AppError;

    let exe = std::env::current_exe()
        .map_err(|e| AppError::Message(format!("无法获取程序路径以注册深链: {e}")))?;
    let exe = exe.display().to_string();

    for scheme in PROTOCOL_SCHEMES {
        register_one_scheme(scheme, &exe);
    }

    Ok(())
}

/// Register a single scheme, logging (never failing) on error.
///
/// # Why these three values, and why all three are load-bearing
///
/// 1. the description in the default value — cosmetic
/// 2. `URL Protocol`, an empty REG_SZ — the one that makes Windows treat the key
///    as a protocol handler at all. Without it the key is just a file
///    association for something named `<scheme>`, the shell will not dispatch
///    `<scheme>://`, and nothing whatsoever happens when a user clicks the
///    button in their browser
/// 3. the open command
///
/// A failure here means deep links are simply gone, so it is logged rather than
/// swallowed.
///
/// # Why the registry API and not `reg add`
///
/// This runs on every launch, and each `reg add` is a child process. The app is
/// built for the Windows GUI subsystem, and a GUI process that starts a console
/// child without `CREATE_NO_WINDOW` gets a fresh console window allocated for
/// it. With two schemes and three values per scheme that was **six console
/// windows on every start** — precisely the "several windows flash when it
/// opens" the owner reported. Writing through `winreg` keeps the whole job
/// in-process: no child, no console, nothing on screen.
///
/// It also stores the command string verbatim. `reg add /d '"…exe" "%1"'` was
/// measured dropping the surrounding quotes, which survives a path with no
/// spaces and breaks one with them (`C:\Program Files\…`) — the same thing
/// `scripts\restore_registration.ps1` works around.
#[cfg(target_os = "windows")]
fn register_one_scheme(scheme: &str, exe: &str) {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let base = format!(r"Software\Classes\{scheme}");

    // (what it is, key path, value name, value). An empty value name is the key's
    // default value — the one Explorer reads for the description and, under
    // `shell\open\command`, the command it runs.
    let steps: [(&str, String, &str, String); 3] = [
        (
            "协议",
            base.clone(),
            "",
            format!("URL:AimyriA go {scheme} Protocol"),
        ),
        (
            "URL Protocol 标记",
            base.clone(),
            "URL Protocol",
            // Empty on purpose: Windows only tests for the value's presence.
            String::new(),
        ),
        (
            "打开命令",
            format!(r"{base}\shell\open\command"),
            "",
            format!("\"{exe}\" \"%1\""),
        ),
    ];

    for (label, path, name, value) in steps {
        // `create_subkey` both creates the key and opens it for writing, so each
        // step stands on its own and their order does not matter.
        match hkcu
            .create_subkey(&path)
            .and_then(|(key, _)| key.set_value(name, &value))
        {
            Ok(()) => {}
            Err(e) => log::warn!("注册 {scheme}:// {label} 失败: {e}"),
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn register_protocol(_app: &AppHandle) -> Result<(), crate::error::AppError> {
    Ok(())
}

/// Record that the user turned start-on-login off, so [`maybe_run`] leaves it
/// alone from now on.
pub fn mark_autostart_opted_out(state: &AppState) -> Result<(), crate::error::AppError> {
    state.db.set_setting(AUTOSTART_SKIPPED, "1")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// These strings are the only record that a user opted out. Renaming one
    /// would silently re-enable autostart for every existing user who had
    /// turned it off — the tool would overwrite the machine with a decision
    /// they had already made against.
    #[test]
    fn the_opt_out_key_is_stable() {
        assert_eq!(AUTOSTART_SKIPPED, "autostart_user_opted_out");
    }

    /// There is deliberately no once-only marker left in this module. That is
    /// the property the last two bugs were both instances of: a bootstrap
    /// gated on "already ran" that stopped re-asserting state an uninstall had
    /// removed. If a marker is ever reintroduced, it has to be about something
    /// that is genuinely one-time — a notice, say — and never about making the
    /// machine match this tool's requirements.
    #[test]
    fn no_once_only_marker_is_left() {
        // Assembled at runtime rather than written as one literal: the search
        // string has to not appear in this file, and a test that names the
        // thing it forbids has just failed on its first run.
        let marker = ["first_run_bootstrap_do", "ne"].concat();
        let source = include_str!("first_run.rs");
        assert!(
            !source.contains(&marker),
            "the once-only bootstrap marker came back; see the module docs",
        );
    }

    /// Autostart stays off once the user turns it off — the property that makes
    /// a default-on setting acceptable.
    #[test]
    fn opting_out_is_recorded_so_a_later_launch_respects_it() {
        // A real Database rather than a stub: the whole guarantee rests on this
        // write persisting and being readable on the next launch, and a stub
        // would only prove the call was made.
        let state = AppState::new(Arc::new(
            crate::database::Database::memory().expect("in-memory database"),
        ));

        mark_autostart_opted_out(&state).expect("record the opt-out");

        assert_eq!(
            state.db.get_setting(AUTOSTART_SKIPPED).unwrap().as_deref(),
            Some("1"),
            "the opt-out must be readable on the next launch",
        );
    }

    /// This module must never start a child process.
    ///
    /// It runs on every launch, and the app is a Windows GUI binary: any console
    /// child started from here without `CREATE_NO_WINDOW` gets its own console
    /// window. That is exactly what shipped once — six `reg add` children, six
    /// windows flashing on screen at every start. The registration goes through
    /// the registry API precisely so there is nothing to hide.
    ///
    /// Written as a source scan rather than a runtime spy because the defect is
    /// "a call site exists", not "a call site ran" — an early-return path would
    /// make a runtime check pass while the next launch still flashes.
    #[test]
    fn the_module_never_spawns_a_child_process() {
        // Assembled at runtime: a test that names the forbidden call in one
        // literal would match itself and fail on its first run.
        let forbidden = ["Command", "::new"].concat();
        let source = include_str!("first_run.rs");
        assert!(
            !source.contains(&forbidden),
            "first_run spawns a child process again; that is a console window on \
             every launch — use the registry API instead",
        );
    }
}
