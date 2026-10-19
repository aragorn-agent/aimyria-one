// M3 phase 1 · batch 11.
'use strict';
const fs = require('fs');
const R = 'D:\\src\\cc-switch-main\\src-tauri\\src\\';
const read = f => fs.readFileSync(R + f, 'utf8').split(/\r?\n/);
const write = (f, L) => { fs.writeFileSync(R + f, L.join('\n'), 'utf8'); console.log(`  ${f}: ${L.length}`); };
const at = (L, t, from = 0) => { for (let i = from; i < L.length; i++) if (L[i].includes(t)) return i; return -1; };
const rep = (L, a, b, text) => L.splice(a, b - a + 1, ...text.split('\n'));
const dropRe = (L, re) => { for (let i = L.length - 1; i >= 0; i--) if (re.test(L[i])) L.splice(i, 1); };

// ── editor_types.rs: rebuild the tail cleanly ──────────────────────────
{
  const L = read('services\\provider\\editor_types.rs');
  const i = at(L, 'pub enum ConflictPolicy {');
  if (i >= 0) {
    const j = L.length - 1;
    while (j > i && L[j].trim() === '') j--;
    rep(L, i, j, `pub enum ConflictPolicy {
    /// Report the conflict and write nothing.
    #[default]
    Refuse,
    /// Overwrite with the editor's value.
    KeepMine,
    /// Keep the external value for conflicting keys; apply the rest.
    KeepTheirs,
}

impl ConflictPolicy {
    /// Decide which changes may be written, given what is on disk now.
    ///
    /// A change conflicts when the live value differs both from what the editor
    /// started from *and* from what it would write — i.e. something else
    /// changed that key while the editor was open.
    pub fn resolve<T, F>(
        &self,
        changes: &[T],
        mut conflict_of: F,
    ) -> Result<(), Vec<String>>
    where
        F: FnMut(&T) -> Option<String>,
    {
        let conflicts: Vec<String> = changes.iter().filter_map(&mut conflict_of).collect();
        match self {
            ConflictPolicy::Refuse if !conflicts.is_empty() => Err(conflicts),
            _ => Ok(()),
        }
    }
}

/// A save coming from the editor UI.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSave {
    /// The full config as displayed when the editor opened.
    pub base: Value,
    /// For add dialogs: the draft the display was projected from.
    #[serde(default)]
    pub draft: Option<Value>,
    #[serde(default)]
    pub on_conflict: ConflictPolicy,
}`);
  }
  write('services\\provider\\editor_types.rs', L);
}

// ── commands/provider.rs ─────────────────────────────────────────────────
{
  const L = read('commands\\provider.rs');
  const s = at(L, '    let _ = (app_type, settingsConfig, category, providerId, meta);');
  if (s >= 0) L[s] = '    let _ = (settingsConfig, category, providerId, meta);';
  write('commands\\provider.rs', L);
}

// ── model_fetch.rs: no `self` in this free function ─────────────────────
{
  const L = read('services\\model_fetch.rs');
  for (let i = 0; i < L.length; i++) {
    if (L[i].includes('self.timeout_secs')) L[i] = L[i].replace('self.timeout_secs', 'timeout_secs');
  }
  write('services\\model_fetch.rs', L);
}

// ── mod.rs ───────────────────────────────────────────────────────────────
{
  const L = read('services\\provider\\mod.rs');
  if (at(L, 'const USAGE_SCRIPT_UNSUPPORTED') < 0) {
    L.unshift('/// Shown wherever the removed usage-script runner used to be called.',
      'const USAGE_SCRIPT_UNSUPPORTED: &str = "该供应商未配置用量查询脚本";',
      '');
  }
  // the codex snippet extractor lost its body with the file-level functions
  if (at(L, 'fn extract_codex_common_config(') < 0) {
    const i = at(L, '    /// Extract common config for Gemini');
    L.splice(i, 0,
      '    /// The Codex config.toml fields safe to share across providers.',
      '    ///',
      '    /// Drops the route table, MCP blocks and the per-provider auth, keeping',
      '    /// only settings that describe the upstream rather than which provider.',
      '    fn extract_codex_common_config(settings: &Value) -> Result<String, AppError> {',
      '        let config_toml = settings',
      '            .get("config")',
      '            .and_then(Value::as_str)',
      '            .unwrap_or("");',
      '        if config_toml.trim().is_empty() {',
      '            return Ok(String::new());',
      '        }',
      '        let Ok(doc) = config_toml.parse::<toml_edit::DocumentMut>() else {',
      '            return Ok(String::new());',
      '        };',
      '        let mut doc = doc;',
      '        let root = doc.as_table_mut();',
      '        root.remove("model_providers");',
      '        root.remove("mcp_servers");',
      '        Ok(doc.to_string().trim().to_string())',
      '    }',
      '');
  }
  write('services\\provider\\mod.rs', L);
}

// ── lib.rs: two calls into helpers that no longer exist ─────────────────
{
  const L = read('lib.rs');
  for (let i = 0; i < L.length; i++) {
    if (L[i].includes('crate::services::provider::import_default_config(')) {
      L[i] = L[i].replace('crate::services::provider::import_default_config(',
        'crate::services::provider::ProviderService::import_default_config(');
    }
  }
  dropRe(L, /^\s*tray::note_tray_click\(\*button\);$/);
  write('lib.rs', L);
}

// ── tray.rs: the subscription cache is gone ────────────────────────────
{
  const L = read('tray.rs');
  dropRe(L, /usage_cache\.invalidate_subscription\(app_type\);/);
  write('tray.rs', L);
}

// ── stream_check.rs ─────────────────────────────────────────────────────
{
  const L = read('services\\stream_check.rs');
  // only OpenCode/OpenClaw/Hermes extractors survived the cuts
  for (const [app, expr] of [
    ['Claude', 'Self::resolve_claude_base_url(provider)'],
    ['Codex', 'Self::resolve_codex_base_url(provider)'],
    ['Gemini', 'Self::resolve_gemini_base_url(provider)'],
    ['GrokBuild', 'Self::resolve_grok_base_url(provider)'],
  ]) {
    for (let i = 0; i < L.length; i++) {
      const m = L[i].match(new RegExp(`AppType::${app} => Self::extract_${app.toLowerCase()}_base_url\\(provider\\),?`));
      if (m) L[i] = L[i].replace(m[0], `AppType::${app} => ${expr},`);
    }
  }
  // build_result yields a value; the caller must not wrap it
  const s = at(L, '        Self::build_result(');
  if (s >= 0 && L[s - 1].trim() === 'Ok(') L[s - 1] = L[s - 1].slice(0, -3).trimEnd();
  write('services\\stream_check.rs', L);
}

// ── editor_toml.rs: apply_to returns Result, callers must not double-wrap ─
{
  const L = read('services\\provider\\editor_toml.rs');
  const s = at(L, 'self.apply_to(');
  if (s >= 0) L[s - 1] = L[s - 1].replace('        self.apply_to(', '        self.apply_to(');
  write('services\\provider\\editor_toml.rs', L);
}

// ── usage_stats.rs ──────────────────────────────────────────────────────
{
  const L = read('services\\usage_stats.rs');
  if (at(L, 'impl ModelPricing') < 0) {
    const i = at(L, 'pub struct ModelPricing {');
    let e = -1;
    for (let k = i; k < L.length; k++) if (L[k] === '}') { e = k; break; }
    rep(L, e + 1, e + 1, `
impl ModelPricing {
    pub fn from_strings(
        model_id: &str,
        input: &str,
        output: &str,
        cache_read: &str,
        cache_creation: &str,
    ) -> Self {
        let num = |v: &str| v.parse::<f64>().unwrap_or(0.0);
        Self {
            model_id: model_id.to_string(),
            input_price: num(input),
            output_price: num(output),
            cache_read_price: num(cache_read),
            cache_creation_price: num(cache_creation),
        }
    }
}`);
  }
  write('services\\usage_stats.rs', L);
}

// ── speedtest.rs ────────────────────────────────────────────────────────
{
  const L = read('services\\speedtest.rs');
  for (let i = 0; i < L.length; i++) {
    if (L[i].includes('sanitize_timeout(') && L[i].includes('.unwrap_or(')) {
      L[i] = L[i].replace('.unwrap_or(10)', '');
    }
  }
  write('services\\speedtest.rs', L);
}

// ── codex_config.rs ─────────────────────────────────────────────────────
{
  const L = read('codex_config.rs');
  for (let i = 0; i < L.length; i++) {
    if (/find_codex_official_model\(model, official\)/.test(L[i])) {
      L[i] = L[i].replace('find_codex_official_model(model, official)', 'find_codex_official_model(&model, official)');
    }
  }
  write('codex_config.rs', L);
}

// ── commands/model_fetch.rs ─────────────────────────────────────────────
{
  const L = read('commands\\model_fetch.rs');
  const s = at(L, '.collect::<Vec<_>>()');
  write('commands\\model_fetch.rs', L);
}

console.log('done');