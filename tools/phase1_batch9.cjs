// M3 phase 1 · batch 9.
'use strict';
const fs = require('fs');
const R = 'D:\\src\\cc-switch-main\\src-tauri\\src\\';
const read = f => fs.readFileSync(R + f, 'utf8').split(/\r?\n/);
const write = (f, L) => { fs.writeFileSync(R + f, L.join('\n'), 'utf8'); console.log(`  ${f}: ${L.length}`); };
const at = (L, t, from = 0) => { for (let i = from; i < L.length; i++) if (L[i].includes(t)) return i; return -1; };
const rep = (L, a, b, text) => L.splice(a, b - a + 1, ...text.split('\n'));
const dropRe = (L, re) => { for (let i = L.length - 1; i >= 0; i--) if (re.test(L[i])) L.splice(i, 1); };

// ── commands/provider.rs ─────────────────────────────────────────────────
{
  const L = read('commands\\provider.rs');
  dropRe(L, /^\s*&xai_state,$/);
  dropRe(L, /^\s*&copilot_state,$/);

  // get_provider_editor_view: the editor lands in phase 2
  const s = at(L, 'let category = ProviderService::editor_category(');
  if (s >= 0) {
    let e = -1;
    for (let i = s; i < L.length; i++) if (L[i].trim() === '})') { e = i; break; }
    rep(L, s - 1, e + 1, `        Err("编辑器在后续版本提供".to_string())`);
  }
  write('commands\\provider.rs', L);
}

// ── editor_toml.rs: the import I dropped is still needed ────────────────
{
  const L = read('services\\provider\\editor_toml.rs');
  if (at(L, 'use crate::error::AppError;') < 0) {
    const i = L.findIndex(l => l.startsWith('use '));
    L.splice(i, 0, 'use crate::error::AppError;');
  }
  write('services\\provider\\editor_toml.rs', L);
}

// ── mod.rs: the usage-script message constant ────────────────────────────
{
  const L = read('services\\provider\\mod.rs');
  if (at(L, 'const USAGE_SCRIPT_UNSUPPORTED') < 0) {
    const i = L.findIndex(l => l.startsWith('impl ProviderService {'));
    L.splice(i, 0,
      '/// Shown wherever the removed usage-script runner used to be called.',
      'const USAGE_SCRIPT_UNSUPPORTED: &str = "该供应商未配置用量查询脚本";',
      '');
  }
  write('services\\provider\\mod.rs', L);
}

// ── stream_check.rs: build_result returns the value directly ────────────
{
  const L = read('services\\stream_check.rs');
  const s = at(L, 'Self::build_result(');
  if (s >= 0 && L[s - 1].trim() === 'Ok(') L[s - 1] = L[s - 1].replace('Ok(', '');
  write('services\\stream_check.rs', L);
}

// ── model_fetch.rs: build the request per candidate URL ─────────────────
{
  const L = read('services\\model_fetch.rs');
  const s = at(L, 'let response = match request.send().await {');
  if (s >= 0) {
    L.splice(s, 0,
      '        let client = match reqwest::Client::builder()',
      '            .timeout(std::time::Duration::from_secs(timeout_secs))',
      '            .build()',
      '        {',
      '            Ok(c) => c,',
      '            Err(e) => return Err(format!("Failed to build HTTP client: {e}")),',
      '        };',
      '        let request = client.get(url);',
      '');
  }
  write('services\\model_fetch.rs', L);
}

// ── speedtest.rs: clamp the caller-supplied timeout ─────────────────────
{
  const L = read('services\\speedtest.rs');
  const i = L.findIndex(l => l.trim() === 'impl SpeedtestService {');
  if (i >= 0) {
    L.splice(i + 1, 0,
      '    /// Clamp a caller-supplied timeout into a range a test can honour.',
      '    fn sanitize_timeout(seconds: u64) -> std::time::Duration {',
      '        std::time::Duration::from_secs(seconds.clamp(1, 30))',
      '    }',
      '');
  }
  write('services\\speedtest.rs', L);
}

// ── usage_stats.rs: ModelPricing lives in this module ───────────────────
{
  const L = read('services\\usage_stats.rs');
  if (at(L, 'pub struct ModelPricing') < 0) {
    const i = L.findIndex(l => l.startsWith('pub(crate) fn find_model_pricing('));
    L.splice(i, 0,
      '/// Per-model price row, as stored in the pricing tables.',
      '#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]',
      'pub struct ModelPricing {',
      '    pub model_id: String,',
      '    pub input_price: f64,',
      '    pub output_price: f64,',
      '    pub cache_read_price: f64,',
      '    pub cache_creation_price: f64,',
      '}',
      '');
  }
  write('services\\usage_stats.rs', L);
}

// ── database/migration.rs: prompts + skills migration left the build ─────
{
  const L = read('database\\migration.rs');
  for (const sig of ['    fn migrate_prompts(', '    fn migrate_skills(']) {
    const s = at(L, sig);
    if (s < 0) continue;
    let e = -1;
    for (let i = s; i < L.length; i++) if (L[i] === '    }') { e = i; break; }
    rep(L, s, e, sig.replace(/fn (\w+)\(/, 'fn $1(') + `
        _tx: &rusqlite::Transaction<'_>,
        _config: &MultiAppConfig,
    ) -> Result<(), AppError> {
        // Prompts and Skills were removed in M2, so there is nothing to carry
        // across. The tables are left alone: dropping them is a separate,
        // deliberate migration.
        Ok(())
    }`);
  }
  write('database\\migration.rs', L);
}

console.log('done');