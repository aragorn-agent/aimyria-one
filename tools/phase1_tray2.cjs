// M3 phase 1 · tray.rs: restore the function bodies that block-cutting emptied.
//
// Each replacement below is the smallest body that is actually correct for
// this build. Where upstream depended on a removed subsystem (local proxy,
// profiles, per-app switcher), the replacement drops that dependency rather
// than faking the value.
'use strict';
const fs = require('fs');
const p = 'D:\\src\\cc-switch-main\\src-tauri\\src\\tray.rs';
let L = fs.readFileSync(p, 'utf8').split(/\r?\n/);

const at = (txt, from = 0) => { for (let i = from; i < L.length; i++) if (L[i].includes(txt)) return i; return -1; };
function replace(a, b, text) {
  if (a < 0 || b < a) { console.error(`BAD RANGE ${a}-${b} for "${text.slice(0, 40)}"`); process.exit(1); }
  L.splice(a, b - a + 1, ...text.split('\n'));
}

// ── provider_entry: the struct literal ended the function; drop the orphan ──
{
  const s = at('    refresh_tray_menu(app);', at('fn provider_entry(app: &AppType'));
  replace(s, s + 1, '');
}

// ── clear_app_problems ─────────────────────────────────────────────────────
{
  const s = at('fn clear_app_problems(app: &AppType) -> bool {');
  const e = at('}', s);
  replace(s, e, `fn clear_app_problems(app: &AppType) -> bool {
    let mut guard = PROBLEMS.lock().unwrap();
    let before = guard.len();
    guard.retain(|problem| &problem.app != app);
    let removed = guard.len() != before;
    drop(guard);
    if removed {
        APP_ATTENTION.fetch_and_set(app, false);
    }
    removed
}`);
}

// ── record_switch_failure ──────────────────────────────────────────────────
{
  const s = at('fn record_switch_failure(app: &AppType, reason: String) {');
  const e = at('}', s);
  replace(s, e, `fn record_switch_failure(app: &AppType, reason: String) {
    let mut guard = PROBLEMS.lock().unwrap();
    guard.retain(|problem| &problem.app != app);
    guard.push(TrayProblem {
        app: app.clone(),
        kind: TrayProblemKind::SwitchFailed,
        reason,
    });
    drop(guard);
    APP_ATTENTION.fetch_and_set(app, true);
}`);
}

// ── service_running: no proxy service in this build ────────────────────────
{
  const s = at('fn service_running(app_state: &AppState) -> Option<bool> {');
  const e = at('}', s);
  replace(s, e, `fn service_running(_app_state: &AppState) -> Option<bool> {
    // We run no local routing service, so there is nothing that can be down.
    // Reported as \`Some(true)\` so callers that treat \`None\` as "unknown" keep
    // treating the tool as healthy.
    Some(true)
}`);
}

// ── current_feedback ───────────────────────────────────────────────────────
{
  const s = at('fn current_feedback() -> Option<TrayFeedback> {');
  const e = at('}', s);
  replace(s, e, `fn current_feedback() -> Option<TrayFeedback> {
    FEEDBACK
        .lock()
        .unwrap()
        .as_ref()
        .filter(|record| record.at.elapsed() < FEEDBACK_TTL)
        .map(|record| record.feedback.clone())`);
}

// ── app_row_title: quota_title helper ──────────────────────────────────────
{
  const s = at('fn app_row_title(texts: &TrayTexts, snapshot: &AppSnapshot) -> String {');
  L.splice(s, 0,
    '/// One-line quota summary for the app row, plus whether it is nearly out.',
    '///',
    '/// \`almost_out\` is a separate flag rather than a substring test so the',
    '/// caller can style "余额偏低" differently from the amount itself.',
    'fn quota_title(texts: &TrayTexts, snapshot: &AppSnapshot) -> Option<(String, bool)> {',
    '    let lines = match snapshot.quota.as_ref()? {',
    '        QuotaView::Lines(lines) if !lines.is_empty() => lines,',
    '        _ => return None,',
    '    };',
    '    let title = lines',
    '        .iter()',
    '        .map(|line| line.text.as_str())',
    '        .collect::<Vec<_>>()',
    '        .join(", ");',
    '    let almost_out = lines',
    '        .iter()',
    '        .any(|line| line.left.is_finite() && line.left < WARN_BELOW_PERCENT);',
    '    let _ = texts;',
    '    Some((title, almost_out))',
    '}',
    '');
}

// ── quota_note ─────────────────────────────────────────────────────────────
{
  const s = at('fn app_children(');
  L.splice(s, 0,
    '/// Longer quota explanation shown as its own clickable row.',
    'fn quota_note(texts: &TrayTexts, quota: &QuotaView, _now: chrono::DateTime<chrono::Local>) -> Option<String> {',
    '    let lines = match quota {',
    '        QuotaView::Lines(lines) => lines,',
    '        QuotaView::Failed(_) => return None,',
    '    };',
    '    if lines.is_empty() {',
    '        return None;',
    '    }',
    '    let mut note = lines',
    '        .iter()',
    '        .filter_map(|line| match &line.resets_at {',
    '            Some(at) => Some(format!("{} · {}", line.text, at)),',
    '            None => Some(line.text.clone()),',
    '        })',
    '        .collect::<Vec<_>>()',
    '        .join("\\n");',
    '    note = truncate_chars(&note, 200);',
    '    let _ = texts;',
    '    Some(note)',
    '}',
    '');
}

// ── collect_snapshots: no profiles, no proxy endpoint ──────────────────────
{
  const s = at('fn collect_snapshots(');
  const e = at('fn collect_status(', s);
  replace(s, e - 1, `fn collect_snapshots(
    app_state: &AppState,
    texts: &TrayTexts,
) -> Result<(Vec<AppSnapshot>, Vec<TrayProblem>), AppError> {
    let settings = crate::settings::get_settings();
    let visible_apps = settings.visible_apps.clone().unwrap_or_default();

    let mut snapshots: Vec<AppSnapshot> = Vec::new();
    for app in TRAY_APPS.iter().filter(|app| visible_apps.is_visible(app)) {
        let providers = app_state.db.get_all_providers(app.as_str()).unwrap_or_default();
        let entries: Vec<ProviderEntry> = providers
            .values()
            .map(|provider| provider_entry(app, provider))
            .collect();
        let current = app_state
            .db
            .get_all_providers(app.as_str())
            .ok()
            .and_then(|all| crate::settings::get_current_provider(app).and_then(|id| all.get(&id).cloned()))
            .map(|provider| provider_entry(app, &provider));
        snapshots.push(AppSnapshot {
            app: app.clone(),
            mode: if current.as_ref().is_some_and(|p| p.needs_routing) {
                TrayMode::Desktop
            } else {
                TrayMode::Direct
            },
            providers: entries,
            current,
            quota: usage_view(
                &app_state.usage_cache,
                texts,
                app,
                app_state
                    .db
                    .get_all_providers(app.as_str())
                    .ok()
                    .and_then(|all| {
                        crate::settings::get_current_provider(app)
                            .and_then(|id| all.get(&id).cloned())
                    })
                    .as_ref()
                    .unwrap_or(&Provider::with_id(
                        String::new(),
                        String::new(),
                        Value::Null,
                        None,
                    )),
                &crate::settings::get_current_provider(app).unwrap_or_default(),
            ),
            service_down: false,
            needs_attention: APP_ATTENTION.load(app),
            profiles: Vec::new(),
        });
    }

    let visible: Vec<(AppType, bool)> = snapshots
        .iter()
        .map(|snapshot| (snapshot.app.clone(), snapshot.mode == TrayMode::Direct))
        .collect();
    // We run no local routing service, so there is no port to check.
    let problems = collect_problems(None, &visible);
    mark_attention(&mut snapshots, &problems);
    Ok((snapshots, problems))
}
`);
}

// ── routing_in_use ─────────────────────────────────────────────────────────
{
  const s = at('fn routing_in_use(app_state: &AppState) -> bool {');
  const e = at('}', s);
  replace(s, e, `fn routing_in_use(_app_state: &AppState) -> bool {
    // No local routing service exists in this build.
    false
}`);
}

// ── update_problem_indicator ───────────────────────────────────────────────
{
  const s = at('    let icon_changed = {');
  const e = at('    };', s);
  replace(s, e, `    let icon_changed = {
        let mut slot = PROBLEM_INDICATOR.lock().unwrap();
        let changed = *slot != Some(has_dot);
        *slot = Some(has_dot);
        changed
    }`);
}

// ── update_tray_usage_labels: rebuild the submenu text ────────────────────
{
  const s = at('        for snapshot in &snapshots {');
  const e = at('        }', s);
  replace(s, e, `        for snapshot in &snapshots {
            let submenu = TRAY_SECTION_SUBMENUS.lock().unwrap().get(&snapshot.app).cloned();
            let Some(submenu) = submenu else { continue };
            if let Err(e) = submenu.set_text(app_row_title(&texts, snapshot)) {
                log::debug!(
                    "[Tray] 更新{}子菜单标题失败: {e}",
                    snapshot.app.as_str()
                );
            }
        }`);
}

// ── take_tray_navigation ──────────────────────────────────────────────────
{
  const s = at('pub fn take_tray_navigation() -> Option<TrayNavigation> {');
  const e = at('}', s);
  replace(s, e, `pub fn take_tray_navigation() -> Option<TrayNavigation> {
    let pending = PENDING_NAVIGATION.lock().unwrap().take();
    pending
}`);
}

fs.writeFileSync(p, L.join('\n'), 'utf8');
console.log(`tray.rs -> ${L.length} lines`);