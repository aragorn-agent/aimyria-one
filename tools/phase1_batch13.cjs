// M3 phase 1 · batch 13 — clearing the last errors.
'use strict';
const fs = require('fs');
const R = 'D:\\\\src\\\\cc-switch-main\\\\src-tauri\\\\src\\\\';

const read = f => fs.readFileSync(R + f, 'utf8').split(/\r?\n/);
const write = (f, L) => { fs.writeFileSync(R + f, L.join('\n'), 'utf8'); console.log('  ' + f + ': ' + L.length); };
const at = (L, t, from = 0) => { for (let i = from; i < L.length; i++) if (L[i].includes(t)) return i; return -1; };
const rep = (L, a, b, text) => L.splice(a, b - a + 1, ...text.split('\n'));
const edit = (f, fn) => { const L = read(f); fn(L); write(f, L); };

// ── stream_check.rs ──────────────────────────────────────────────────────
edit('services\\\\stream_check.rs', L => {
  // build_result yields a value; the enclosing fn returns a Result
  const s = at(L, '        Self::build_result(');
  if (s >= 0) L[s] = '        Ok(Self::build_result(';
  const c = at(L, '            config.degraded_threshold_ms,', s);
  if (c >= 0) {
    let e = c;
    while (e < L.length && L[e].trim() !== ')') e++;
    L[e] = L[e] + ')';
  }

  // pass the app's string key, not the enum variant
  for (const [variant, key] of [['Claude', 'claude'], ['Codex', 'codex'], ['Gemini', 'gemini']]) {
    const bad = 'Self::extract_env_base_url(provider, ' + variant + ')';
    const i = L.findIndex(l => l.includes(bad));
    if (i >= 0) L[i] = L[i].replace(bad, 'Self::extract_env_base_url(provider, "' + key + '")');
  }

  // add the helper next to the other extractors
  if (at(L, 'fn extract_env_base_url(') < 0) {
    const j = at(L, '    fn extract_openclaw_base_url(');
    L.splice(j, 0, ...[
      '    /// Read the upstream base URL out of a provider row for the apps that',
      '    /// keep it in an `env` map.',
      '    fn extract_env_base_url(provider: &Provider, app: &str) -> Result<String, AppError> {',
      '        let key = match app {',
      '            "claude" => "ANTHROPIC_BASE_URL",',
      '            "codex" | "grokbuild" => "OPENAI_BASE_URL",',
      '            "gemini" => "GOOGLE_GEMINI_BASE_URL",',
      '            other => {',
      '                return Err(AppError::Message(format!("{other} 不支持连通性检测")));',
      '            }',
      '        };',
      '        provider',
      '            .settings_config',
      '            .get("env")',
      '            .and_then(|v| v.get(key))',
      '            .and_then(|v| v.as_str())',
      '            .map(str::trim)',
      '            .filter(|s| !s.is_empty())',
      '            .map(str::to_string)',
      '            .ok_or_else(|| AppError::Message(format!("供应商未配置 {key}，无法检测连通性")))',
      '    }',
      '',
    ]);
  }
});

// ── tray.rs: compute the quota view before the struct literal consumes
//    `current_id`, so no clone-rename dance is needed ─────────────────────
edit('tray.rs', L => {
  const hack = at(L, '        let __current_id = current_id.clone();');
  if (hack >= 0) L.splice(hack, 1);
  const field = at(L, '            __current_id,');
  if (field >= 0) L[field] = '            current_id,';

  // pull `quota: usage_view(...)` out of the literal and hoist it
  const q = at(L, '            quota: usage_view(');
  if (q >= 0) {
    let e = q;
    let d = 0;
    for (let i = q; i < L.length; i++) {
      for (const ch of L[i]) { if (ch === '(') d++; else if (ch === ')') d--; }
      if (d === 0 && i > q) { e = i; break; }
    }
    const block = L.splice(q, e - q + 1);
    // re-indent by 4 less
    for (let i = 0; i < block.length; i++) {
      block[i] = i === 0 ? block[i].replace('quota: usage_view(', 'let quota = usage_view(') : block[i];
      if (i > 0) block[i] = block[i].replace(/^ {4}/, '');
    }
    const push = at(L, '        snapshots.push(AppSnapshot {');
    L.splice(push, 0, ...block, '');
    const q2 = at(L, '            quota: usage_view(');
    if (q2 >= 0) L[q2] = '            quota,';
  }
});

// ── usage_stats.rs: from_strings takes the model id too, and is infallible
edit('services\\\\usage_stats.rs', L => {
  const s = at(L, 'ModelPricing::from_strings(&input, &output, &cache_read, &cache_creation).ok()');
  if (s >= 0) {
    L[s] = '            ModelPricing::from_strings(model_id, &input, &output, &cache_read, &cache_creation)';
  }
});

// ── codex_config.rs: two helpers take &str ────────────────────────────────
edit('codex_config.rs', L => {
  const s = at(L, '.is_some_and(|found| is_codex_official_mirror(entry, model, found))');
  if (s >= 0) L[s] = L[s].replace('entry, model, found', 'entry, &model, found');
  const c = at(L, 'codex_catalog_input_modalities(model, None)');
  if (c >= 0) L[c] = L[c].replace('(model, None)', '(&model, None)');
});

// ── speedtest.rs: build_client lost its body ────────────────────────────
edit('services\\\\speedtest.rs', L => {
  const s = at(L, '    fn build_client(timeout_secs: u64) -> Result<(Client, std::time::Duration), AppError> {');
  if (s >= 0) {
    let e = -1;
    for (let i = s; i < L.length; i++) if (L[i] === '    }') { e = i; break; }
    rep(L, s, e, ...[
      '    fn build_client(timeout_secs: u64) -> Result<(Client, std::time::Duration), AppError> {',
      '        let timeout = std::time::Duration::from_secs(timeout_secs);',
      '        let client = Client::builder()',
      '            .timeout(timeout)',
      '            .build()',
      '            .map_err(|e| AppError::Message(format!("failed to build HTTP client: {e}")))?;',
      '        Ok((client, timeout))',
      '    }',
    ].join('\n').split('\n'));
  }
});

console.log('done');