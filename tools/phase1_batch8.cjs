// M3 phase 1 · batch 8: the last round of individual fixes.
'use strict';
const fs = require('fs');
const R = 'D:\\src\\cc-switch-main\\src-tauri\\src\\';
const read = f => fs.readFileSync(R + f, 'utf8').split(/\r?\n/);
const write = (f, L) => { fs.writeFileSync(R + f, L.join('\n'), 'utf8'); console.log(`  ${f}: ${L.length}`); };
const at = (L, t, from = 0) => { for (let i = from; i < L.length; i++) if (L[i].includes(t)) return i; return -1; };
const rep = (L, a, b, text) => L.splice(a, b - a + 1, ...text.split('\n'));
const dropRe = (L, re) => { for (let i = L.length - 1; i >= 0; i--) if (re.test(L[i])) L.splice(i, 1); };

// ── services/provider/usage.rs: only existed to wrap usage_script ────────
{
  fs.unlinkSync(R + 'services\\provider\\usage.rs');
  const L = read('services\\provider\\mod.rs');
  dropRe(L, /^\s*#\[cfg\(test\)\]$/);
  dropRe(L, /^\s*mod usage;$/);
  write('services\\provider\\mod.rs', L);
}

// ── stream_check.rs ──────────────────────────────────────────────────────
{
  const L = read('services\\stream_check.rs');
  // build_result takes the Result, not the unwrapped status
  const b = at(L, 'Ok(Self::build_result(');
  L[b] = '        Self::build_result(';
  L[b + 1] = '            Ok(status),';
  L[b + 4] = '        )';
  // leftover send block from the pre-rewrite version
  const r = at(L, 'match req.send().await {');
  if (r >= 0) {
    let e = -1;
    for (let i = r; i < L.length; i++) if (L[i] === '    }') { e = i; break; }
    rep(L, r - 2, e, '');
  }
  write('services\\stream_check.rs', L);
}

// ── editor_toml.rs: the live-write half belonged to the removed engine ───
{
  const L = read('services\\provider\\editor_toml.rs');
  const s = at(L, 'pub(crate) fn apply_to(');
  if (s >= 0) {
    let e = -1;
    for (let i = s; i < L.length; i++) if (L[i] === '    }') { e = i; break; }
    if (e > s) rep(L, s, e, `pub(crate) fn apply_to(&self, doc: &mut DocumentMut) {
        let accepted = self.on_conflict.resolve(&self.changes, |change| {
            let current = item_at(doc, &change.path).map(render);
            let after = change.after.as_ref().map(render);
            (current != change.before && current != after).then(|| change.path.join("."))
        });
        match accepted {
            Ok(()) => Ok(()),
            Err(conflicts) => Err(AppError::Message(format!(
                "配置已被其他程序修改，未写入：{}",
                conflicts.join(", ")
            ))),
        }
    }`);
  }
  dropRe(L, /^use crate::error::AppError;$/);
  write('services\\provider\\editor_toml.rs', L);
}

// ── tray.rs: annotate the futures vec ────────────────────────────────────
{
  const L = read('tray.rs');
  const s = at(L, 'let mut usage_futures = Vec::new();');
  if (s >= 0) L[s] = '    let mut usage_futures: Vec<tokio::task::JoinHandle<()>> = Vec::new();';
  write('tray.rs', L);
}

// ── commands/provider.rs ─────────────────────────────────────────────────
{
  const L = read('commands\\provider.rs');
  dropRe(L, /^\s*xai_state: State<'_, XaiOAuthState>,$/);
  // the editor-view commands belong to the phase-2 editor rebuild
  for (const [sig, stub] of [
    ['pub fn editor_view_with_meta(', null],
  ]) { /* handled below */ }
  const e1 = at(L, 'pub fn editor_view_with_meta(');
  if (e1 >= 0) {
    let e = -1;
    for (let i = e1; i < L.length; i++) if (L[i] === '}') { e = i; break; }
    rep(L, e1, e, `pub fn editor_view_with_meta(
    _state: State<'_, AppState>,
    app: String,
    provider_id: String,
    _meta: Option<crate::provider::ProviderMeta>,
) -> Result<crate::services::provider::EditorView, String> {
    let _ = (app, provider_id);
    Err("编辑器在后续版本提供".to_string())
}`);
  }
  const c1 = at(L, 'pub fn editor_category(');
  if (c1 >= 0) {
    let e = -1;
    for (let i = c1; i < L.length; i++) if (L[i] === '}') { e = i; break; }
    rep(L, c1, e, `pub fn editor_category(_app: String) -> Result<Option<String>, String> {
    Ok(None)
}`);
  }
  // universal-provider commands
  const u = at(L, 'pub fn get_universal_providers(');
  if (u >= 0) {
    let last = L.length - 1;
    while (last > u && L[last].trim() === '') last--;
    rep(L, u, last, '');
  }
  write('commands\\provider.rs', L);
}

// ── mod.rs ───────────────────────────────────────────────────────────────
{
  const L = read('services\\provider\\mod.rs');
  for (let i = 0; i < L.length; i++) {
    if (L[i].includes('crate::read_json_file::')) L[i] = L[i].replace('crate::read_json_file::', 'crate::config::read_json_file::');
  }
  // the message constant
  const s = at(L, '    pub async fn query_usage(');
  if (s >= 0 && at(L, 'const USAGE_SCRIPT_UNSUPPORTED') < 0) {
    L.splice(s, 0,
      '/// Shown wherever the removed usage-script runner used to be called.',
      'const USAGE_SCRIPT_UNSUPPORTED: &str = "该供应商未配置用量查询脚本";',
      '');
  }
  // the validate_usage_script call
  const v = at(L, 'validate_usage_script(usage_script)?;');
  if (v >= 0) L.splice(v - 1, 3);
  write('services\\provider\\mod.rs', L);
}

console.log('done');