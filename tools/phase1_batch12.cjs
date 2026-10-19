// M3 phase 1 路 batch 12 鈥?final.
'use strict';
const fs = require('fs');
const R = 'D:\\src\\cc-switch-main\\src-tauri\\src\\';
const read = f => fs.readFileSync(R + f, 'utf8').split(/\r?\n/);
const write = (f, L) => { fs.writeFileSync(R + f, L.join('\n'), 'utf8'); console.log(`  ${f}: ${L.length}`); };
const at = (L, t, from = 0) => { for (let i = from; i < L.length; i++) if (L[i].includes(t)) return i; return -1; };
const rep = (L, a, b, text) => L.splice(a, b - a + 1, ...text.split('\n'));

const edit = (file, fn) => { const L = read(file); fn(L); write(file, L); };

// model_fetch: no timeout parameter on this function
edit('services\\model_fetch.rs', L => {
  const s = at(L, 'from_secs(timeout_secs)');
  if (s >= 0) L[s] = '            .timeout(std::time::Duration::from_secs(20))';
});

// provider/mod.rs: inline the message rather than relying on a module const
edit('services\\provider\\mod.rs', L => {
  for (let i = 0; i < L.length; i++) {
    if (L[i].includes('USAGE_SCRIPT_UNSUPPORTED.to_string()')) {
      L[i] = '        Err(AppError::Message("璇ヤ緵搴斿晢鏈厤缃敤閲忔煡璇㈣剼鏈?.to_string()))';
    }
  }
});

// speedtest: sanitize_timeout takes Option<u64>
edit('services\\speedtest.rs', L => {
  const s = at(L, 'fn sanitize_timeout(');
  if (s >= 0) {
    let e = -1;
    for (let i = s; i < L.length; i++) if (L[i] === '    }') { e = i; break; }
    rep(L, s, e, `fn sanitize_timeout(timeout_secs: Option<u64>) -> std::time::Duration {
        let secs = timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
        std::time::Duration::from_secs(secs.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS))
    }`);
  }
  const b = at(L, 'let (client, request_timeout) = Self::build_client(');
  if (b >= 0) L[b] = '        let (client, request_timeout) = Self::build_client(timeout.as_secs())?;';
  // drop the leftover duplicate tail of the old body
  const d = at(L, '        let timeout = std::time::Duration::from_secs(timeout_secs);');
  if (d >= 0) L.splice(d, 3);
});

// stream_check: build_result already returns a value; the extractors
edit('services\\stream_check.rs', L => {
  const b = at(L, 'Self::build_result(');
  if (b >= 0) {
    let e = b;
    while (e < L.length && !/^\s*\)\s*$/.test(L[e])) e++;
    L[b + 1] = L[b + 1].replace('Ok(status)', 'Ok(status)');
  }
  // route the four apps through the one extractor that survived
  for (const app of ['Claude', 'Codex', 'Gemini']) {
    const re = new RegExp(`AppType::${app} => Self::resolve_${app.toLowerCase()}_base_url\\(provider\\),`);
    for (let i = 0; i < L.length; i++) {
      if (re.test(L[i])) {
        L[i] = L[i].replace(re, `AppType::${app} => Self::extract_env_base_url(provider, ${app}),`);
      }
    }
  }
  for (let i = 0; i < L.length; i++) {
    if (L[i].includes('Self::extract_grok_base_url(provider)')) {
      L[i] = L[i].replace('Self::extract_grok_base_url(provider)', 'Self::extract_env_base_url(provider, "grokbuild")');
    }
  }
  const j = at(L, '    // ===== 鍚勫簲鐢?base_url 鎻愬彇');
  if (j >= 0) {
    L.splice(j, 0,
      '    /// Read the upstream base URL out of a provider row for the apps whose',
      '    /// settings keep it in `env`.',
      '    fn extract_env_base_url(provider: &Provider, app: &str) -> Result<String, AppError> {',
      '        let key = match app {',
      '            "claude" => "ANTHROPIC_BASE_URL",',
      '            "codex" | "grokbuild" => "OPENAI_BASE_URL",',
      '            "gemini" => "GOOGLE_GEMINI_BASE_URL",',
      '            other => {',
      '                return Err(AppError::Message(format!("{other} 涓嶆敮鎸佽繛閫氭€ф娴?)))',
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
      '            .ok_or_else(|| {',
      '                AppError::Message(format!("渚涘簲鍟嗘湭閰嶇疆 {key}锛屾棤娉曟娴嬭繛閫氭€?))',
      '            })',
      '    }',
      '');
  }
});

// editor_toml: apply_to must return its Result
edit('services\\provider\\editor_toml.rs', L => {
  const s = at(L, 'pub(crate) fn apply_to(&self, doc: &mut DocumentMut) {');
  if (s >= 0) L[s] = 'pub(crate) fn apply_to(&self, doc: &mut DocumentMut) -> Result<(), AppError> {';
});

// usage_stats: the surviving call site builds ModelPricing differently
edit('services\\usage_stats.rs', L => {
  const s = at(L, 'ModelPricing::from_strings(');
  if (s >= 0) {
    L.splice(s, s + 6, '        ModelPricing::from_row(model_id, input, output, cache_read, cache_creation)');
  }
  if (at(L, 'pub fn from_strings(') >= 0) {
    const i = at(L, '    pub fn from_strings(');
    let e = L.findIndex((l, k) => k > i && l === '    }');
    rep(L, i, e, `    pub fn from_strings(
        model_id: &str,
        input: &str,
        output: &str,
        cache_read: &str,
        cache_creation: &str,
    ) -> Self {
        Self::from_row(
            model_id,
            &input.to_string(),
            &output.to_string(),
            &cache_read.to_string(),
            &cache_creation.to_string(),
        )
    }

    pub fn from_row(
        model_id: &str,
        input: &String,
        output: &String,
        cache_read: &String,
        cache_creation: &String,
    ) -> Self {
        let num = |v: &String| v.parse::<f64>().unwrap_or(0.0);
        Self {
            model_id: model_id.to_string(),
            input_price: num(input),
            output_price: num(output),
            cache_read_price: num(cache_read),
            cache_creation_price: num(cache_creation),
        }
    }`);
  }
});

// tray.rs
edit('tray.rs', L => {
  // quota_title takes the snapshot's QuotaView, not the snapshot
  const s = at(L, 'snapshot.quota.as_ref().and_then(|q| quota_title(texts, q))');
  if (s >= 0) L[s] = L[s].replace('|q| quota_title(texts, q)', '|q| quota_title(texts, snapshot)');
  // current_id is read before it is moved into the snapshot
  const c = at(L, 'let current_id = crate::settings::get_current_provider(app);');
  if (c >= 0) {
    const d = at(L, 'current_id.as_deref().unwrap_or_default(),', c);
    if (d > c) {
      const tmp = '__current_id';
      L[d] = L[d].replace('current_id.as_deref()', tmp + '.as_deref()');
      for (let i = c; i < d; i++) if (L[i].includes('current_id,')) L[i] = L[i].replace('current_id,', tmp + ',');
      L.splice(c, 0, '        let ' + tmp + ' = current_id.clone();');
    }
  }
});

// provider.rs: Pi must be matched somewhere
edit('provider.rs', L => {
  const s = at(L, 'AppType::OpenClaw |');
  if (s >= 0 && !L[s].includes('Pi')) L[s] = L[s].replace('AppType::OpenClaw |', 'AppType::Pi | AppType::OpenClaw |');
});

console.log('done');
