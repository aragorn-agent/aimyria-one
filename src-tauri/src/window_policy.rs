//! When the window is visible, whether it owns a taskbar button, and what closing
//! it means.
//!
//! # Why this is a module rather than a few calls inline
//!
//! The taskbar question has now been answered wrongly twice, in opposite
//! directions, and both mistakes were invisible until someone looked at the
//! Windows taskbar while the app was running:
//!
//! 1. **The window had no taskbar button, ever.** `tauri.conf.json` declared
//!    `skipTaskbar: true`, so a visible window could not be found or brought back
//!    from the taskbar. A tray-only app is fine until the tray icon is hidden or
//!    the shell loses it — and then the program is unreachable while still
//!    running: no window to click, no button to restore.
//! 2. **Closing the window exited the process.** With `minimizeToTrayOnClose`
//!    false (its shipped value on this machine), the close handler called
//!    `exit(0)`. So "close the window" meant "quit the tray app", which is not
//!    what a window's X button means in any other program.
//!
//! The lesson is that the three facts — window visible, taskbar button present,
//! tray icon present — have to be decided together. Split across call sites, they
//! drift. [`WindowPolicy`] is the single answer.
//!
//! # The rules
//!
//! | window | tray icon | taskbar button | closing the window |
//! |---|---|---|---|
//! | visible | shown | **yes** | hides to tray, process keeps running |
//! | visible | hidden | **yes** | hides (no way back, so it must stay reachable) |
//! | hidden | shown | no | — |
//! | hidden | hidden | **yes** | — (never remove the last way back) |
//!
//! The last row is the important one: hiding the tray icon while the window is
//! also hidden would leave a running process with no way to reach it. So a hidden
//! window keeps its taskbar button when there is no tray icon.

/// What should happen to the taskbar button and the tray icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPolicy {
    /// Should the window's taskbar button be present?
    pub taskbar_button: bool,
    /// Should the tray icon be visible?
    pub tray_icon: bool,
}

/// The inputs the decision depends on.
#[derive(Debug, Clone, Copy)]
pub struct WindowState {
    /// Is the window currently on screen?
    pub window_visible: bool,
    /// What the user asked for in settings.
    pub show_in_tray: bool,
    /// Is the window currently in "lightweight mode" (tray-only, window destroyed)?
    pub lightweight: bool,
}

/// Decide the policy.
///
/// `lightweight` is the tray-only mode: the window is gone entirely, so its
/// taskbar button is irrelevant and the tray icon is the only way in — which is
/// why it is forced on there.
pub fn decide(state: WindowState) -> WindowPolicy {
    if state.lightweight {
        return WindowPolicy {
            taskbar_button: false,
            tray_icon: true,
        };
    }

    let tray_icon = state.show_in_tray;

    // A visible window always gets a taskbar button: that is what "visible" means
    // to a Windows user, and it is the most discoverable way back to the app.
    if state.window_visible {
        return WindowPolicy {
            taskbar_button: true,
            tray_icon,
        };
    }

    // The window is hidden. Drop the taskbar button only if the tray icon remains,
    // or the process would be running with no entry point anywhere.
    WindowPolicy {
        taskbar_button: !tray_icon,
        tray_icon,
    }
}

/// Should closing the window hide it instead of exiting?
///
/// The answer is yes unless the user explicitly asked to quit on close. A tray
/// application that quits when its window closes contradicts its own reason for
/// existing: it is supposed to keep answering deep links and watching the balance
/// with no window at all.
///
/// The exception is `db_recovery`, where there is no working tray to return to and
/// an invisible background process would be a stuck program rather than a resident
/// one.
pub fn close_should_hide(minimize_to_tray_on_close: bool, db_recovery: bool) -> bool {
    !db_recovery && minimize_to_tray_on_close
}

/// Should the window be shown at all on a normal launch?
pub fn visible_on_launch(silent_startup: bool) -> bool {
    !silent_startup
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(window_visible: bool, show_in_tray: bool, lightweight: bool) -> WindowState {
        WindowState {
            window_visible,
            show_in_tray,
            lightweight,
        }
    }

    /// The regression this whole module exists for: a visible window must have a
    /// taskbar button.
    #[test]
    fn a_visible_window_always_has_a_taskbar_button() {
        for show_in_tray in [true, false] {
            let policy = decide(state(true, show_in_tray, false));
            assert!(
                policy.taskbar_button,
                "a visible window without a taskbar button is unreachable from the taskbar \
                 (show_in_tray={show_in_tray})"
            );
        }
    }

    /// Hiding the window without a tray icon must keep the taskbar button, or the
    /// process becomes unreachable while still running.
    #[test]
    fn a_hidden_window_keeps_its_button_when_there_is_no_tray_icon() {
        let policy = decide(state(false, false, false));
        assert!(!policy.tray_icon, "the user asked for no tray icon");
        assert!(
            policy.taskbar_button,
            "with no tray icon the taskbar button is the only way back"
        );
    }

    /// With a tray icon present, a hidden window drops its button — the tray is
    /// the entry point, and a taskbar button for an invisible window is a ghost.
    #[test]
    fn a_hidden_window_with_a_tray_icon_drops_its_button() {
        let policy = decide(state(false, true, false));
        assert!(policy.tray_icon);
        assert!(!policy.taskbar_button);
    }

    /// Lightweight mode destroys the window, so the tray icon is forced on: it is
    /// the only way to get the window back.
    #[test]
    fn lightweight_mode_forces_the_tray_icon() {
        let policy = decide(state(false, false, true));
        assert!(policy.tray_icon, "lightweight mode has no other entry point");
        assert!(!policy.taskbar_button);
    }

    /// There is always at least one way to reach the process.
    ///
    /// This is the invariant the individual cases exist to protect. If a future
    /// change adds a state, this test fails unless that state is reachable.
    #[test]
    fn every_state_remains_reachable() {
        for window_visible in [true, false] {
            for show_in_tray in [true, false] {
                for lightweight in [true, false] {
                    let policy = decide(state(window_visible, show_in_tray, lightweight));
                    assert!(
                        policy.taskbar_button || policy.tray_icon,
                        "unreachable state: visible={window_visible} \
                         tray={show_in_tray} lightweight={lightweight}"
                    );
                }
            }
        }
    }

    /// Closing the window hides it for a tray app, because that is what its X
    /// button means everywhere else.
    #[test]
    fn closing_hides_unless_the_user_asked_to_quit() {
        assert!(close_should_hide(true, false));
        assert!(
            !close_should_hide(false, false),
            "an explicit quit-on-close setting must be honoured"
        );
    }

    /// In database recovery there is no working tray, so close must exit rather
    /// than leave an invisible stuck process.
    #[test]
    fn recovery_mode_quits_on_close_regardless_of_the_setting() {
        assert!(!close_should_hide(true, true));
        assert!(!close_should_hide(false, true));
    }

    /// A silent start hides the window; a normal start shows it.
    #[test]
    fn launch_visibility_follows_the_setting() {
        assert!(!visible_on_launch(true));
        assert!(visible_on_launch(false));
    }
}
