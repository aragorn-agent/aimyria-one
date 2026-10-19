// M3 phase 1 · batch: references to modules that M2 removed.
'use strict';
const fs = require('fs');
const R = 'D:\\src\\cc-switch-main\\src-tauri\\src\\';

const read = f => fs.readFileSync(R + f, 'utf8').split(/\r?\n/);
const write = (f, L) => { fs.writeFileSync(R + f, L.join('\n'), 'utf8'); console.log(`  ${f}: ${L.length} lines`); };
const at = (L, t, from = 0) => { for (let i = from; i < L.length; i++) if (L[i].includes(t)) return i; return -1; };
const rep = (L, a, b, text) => L.splice(a, b - a + 1, ...text.split('\n'));

// ── tray.rs: block expression needs its semicolon ─────────────────────────
{
  const L = read('tray.rs');
  const s = at(L, 'let icon_changed = {');
  let e = -1;
  for (let i = s; i < L.length; i++) if (L[i] === '    }') { e = i; break; }
  if (e < 0) { console.error('tray icon_changed not found'); process.exit(1); }
  L[e] = '    };';
  write('tray.rs', L);
}

// ── commands/env.rs: env-var management went away with the env feature ────
{
  fs.unlinkSync(R + 'commands\\env.rs');
  const L = read('commands\\mod.rs');
  for (let i = L.length - 1; i >= 0; i--) {
    if (L[i].trim() === 'mod env;') L.splice(i, 1);
    if (L[i].trim() === 'pub use env::*;') L.splice(i, 1);
  }
  write('commands\\mod.rs', L);

  const lib = read('lib.rs');
  for (let i = lib.length - 1; i >= 0; i--) {
    if (/^\s*commands::(check_env_conflicts|delete_env_vars|restore_env_vars|restore_env_backup),?\s*$/.test(lib[i])) {
      lib.splice(i, 1);
    }
  }
  write('lib.rs', lib);
}

// ── commands/provider.rs ──────────────────────────────────────────────────
{
  const L = read('commands\\provider.rs');

  // switch_provider: drop the Claude Desktop wrapper that lost its body
  let s = at(L, 'pub async fn switch_provider(');
  let e = L.findIndex((l, k) => k > s && l === '}');
  rep(L, s, e, `pub async fn switch_provider(
    app_handle: tauri::AppHandle,
    app: String,
    id: String,
) -> Result<SwitchResult, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle
            .try_state::<AppState>()
            .ok_or_else(|| "应用状态未就绪".to_string())?;
        switch_provider_internal(state.inner(), app_type, &id)
            .map_err(|e| format!("供应商切换失败: {e}"))
    })
    .await
    .map_err(|e| format!("供应商切换任务执行失败: {e}"))?
}`);

  // queryProviderUsage: drop the two OAuth states
  s = at(L, 'pub async fn queryProviderUsage(');
  for (let i = s; i < s + 14; i++) {
    if (L[i].includes('copilot_state: State<') || L[i].includes('xai_state: State<')) L.splice(i, 1);
  }
  s = at(L, 'let inner = query_provider_usage_inner(');
  for (let i = s + 1; i < s + 6; i++) {
    if (L[i].includes('&copilot_state,') || L[i].includes('&xai_state,')) L.splice(i, 1);
  }

  write('commands\\provider.rs', L);
}

// ── services/usage_stats.rs: proxy + Claude Desktop references ────────────
{
  const L = read('services\\usage_stats.rs');
  for (let i = L.length - 1; i >= 0; i--) {
    if (/^use crate::proxy::/.test(L[i].trim())) L.splice(i, 1);
  }
  write('services\\usage_stats.rs', L);
}

// ── services/provider/editor_toml.rs ─────────────────────────────────────
{
  const L = read('services\\provider\\editor_toml.rs');
  for (let i = 0; i < L.length; i++) {
    if (L[i].includes('use super::claude_editor::')) {
      L[i] = L[i].replace('super::claude_editor::', 'super::editor_types::');
    }
  }
  write('services\\provider\\editor_toml.rs', L);
}

console.log('done');