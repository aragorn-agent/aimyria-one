// M3 phase 1 · tray.rs: fix the bodies written in the previous pass to match
// the file's real types (AppSnapshot fields, the derived-problem model).
'use strict';
const fs = require('fs');
const p = 'D:\\src\\cc-switch-main\\src-tauri\\src\\tray.rs';
let L = fs.readFileSync(p, 'utf8').split(/\r?\n/);

const at = (txt, from = 0) => { for (let i = from; i < L.length; i++) if (L[i].includes(txt)) return i; return -1; };
function replace(a, b, text) {
  if (a < 0 || b < a) { console.error(`BAD RANGE ${a}-${b} :: ${text.slice(0, 44)}`); process.exit(1); }
  L.splice(a, b - a + 1, ...text.split('\n'));
}

// ── clear_app_problems + record_switch_failure, against a real store ──────
{
  const s = at('fn clear_app_problems(app: &AppType) -> bool {');
  const e = at('fn service_running(', s) - 1;
  replace(s, e, `/// Switch failures the user has not acted on yet.
///
/// \`collect_problems\` derives its problems from state that can be re-read at
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
`);
}

// ── collect_snapshots: real AppSnapshot fields, no proxy, no profiles ──────
{
  const s = at('fn collect_snapshots(');
  const e = at('fn collect_status(', s) - 1;
  replace(s, e, `fn collect_snapshots(
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
                Provider::with_id(String::new(), String::new(), Value::Null, None)
            });

        snapshots.push(AppSnapshot {
            app: app.clone(),
            mode: TrayMode::Direct,
            providers,
            current_id,
            // No failover queue and no aggregate stack: both belonged to the
            // local routing feature.
            queue: Vec::new(),
            stack_members: Vec::new(),
            quota: usage_view(
                &app_state.usage_cache,
                texts,
                app,
                &current_provider,
                current_id.as_deref().unwrap_or_default(),
            ),
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
`);
}

fs.writeFileSync(p, L.join('\n'), 'utf8');
console.log(`tray.rs -> ${L.length} lines`);