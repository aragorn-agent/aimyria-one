// M3 phase 1 · batch 7: repair the last cut function bodies, and drop the
// usage-script runner (a phase-2 feature) rather than ship a half-rebuilt one.
'use strict';
const fs = require('fs');
const R = 'D:\\src\\cc-switch-main\\src-tauri\\src\\';
const read = f => fs.readFileSync(R + f, 'utf8').split(/\r?\n/);
const write = (f, L) => { fs.writeFileSync(R + f, L.join('\n'), 'utf8'); console.log(`  ${f}: ${L.length}`); };
const at = (L, t, from = 0) => { for (let i = from; i < L.length; i++) if (L[i].includes(t)) return i; return -1; };
const rep = (L, a, b, text) => L.splice(a, b - a + 1, ...text.split('\n'));
const dropRe = (L, re) => { for (let i = L.length - 1; i >= 0; i--) if (re.test(L[i])) L.splice(i, 1); };

// ── usage_cache.rs: the subscription caches went with the subscription feature
{
  const L = read('services\\usage_cache.rs');
  dropRe(L, /^\s*(pub )?subscription: RwLock</);
  dropRe(L, /^\s*(pub )?codex_oauth: RwLock</);
  dropRe(L, /^\s*pub fn put_subscription/);
  dropRe(L, /^\s*pub fn invalidate_subscription/);
  // the bodies that referenced those fields
  let s = at(L, 'pub fn put_subscription(');
  if (s >= 0) { let e = L.findIndex((l, k) => k > s && l === '    }'); rep(L, s, e, ''); }
  s = at(L, 'pub fn invalidate_subscription(');
  if (s >= 0) { let e = L.findIndex((l, k) => k > s && l === '    }'); rep(L, s, e, ''); }
  write('services\\usage_cache.rs', L);
}

// ── usage_stats.rs ───────────────────────────────────────────────────────
{
  const L = read('services\\usage_stats.rs');
  const c = at(L, 'crate::claude_desktop_config::ONE_M_CONTEXT_MARKER');
  if (c >= 0) L[c] = '        .trim()';
  write('services\\usage_stats.rs', L);
}

// ── services/stream_check.rs: restore the request-sending halves ──────────
{
  const L = read('services\\stream_check.rs');

  // build_result call site: the response is gone, rebuild it from the base URL
  let s = at(L, 'Ok(Self::build_result(');
  rep(L, s, s + 3, `        let status = match req.send().await {
            Ok(resp) => resp.status().as_u16(),
            Err(e) => return Err(Self::map_request_error(e)),
        };
        Ok(Self::build_result(
            status,
            response_time,
            config.degraded_threshold_ms,
        ))`);

  // resolve_base_url lost its Claude/Codex/Gemini arms
  s = at(L, 'match app_type {', at(L, 'fn resolve_base_url('));
  let e = L.findIndex((l, k) => k > s && l.trim() === '}');
  rep(L, s, e, `        match app_type {
            AppType::Claude => Self::extract_claude_base_url(provider),
            AppType::Codex => Self::extract_codex_base_url(provider),
            AppType::Gemini => Self::extract_gemini_base_url(provider),
            AppType::GrokBuild => Self::extract_grok_base_url(provider),
            AppType::OpenCode => {
                let npm = Self::extract_opencode_npm(provider);
                Self::resolve_opencode_base_url(provider, npm.as_deref())
            }
            AppType::OpenClaw => Self::extract_openclaw_base_url(provider),
            AppType::Hermes => Self::extract_hermes_base_url(provider),
            other => Err(AppError::Message(format!(
                "{} 不支持连通性检测",
                other.as_str()
            ))),
        }`);

  write('services\\stream_check.rs', L);
}

// ── usage_script.rs: phase-2 feature, and too damaged to half-rebuild ────
{
  fs.unlinkSync(R + 'usage_script.rs');
  const L = read('services\\mod.rs');
  dropRe(L, /^\s*pub mod usage_script;$/);
  dropRe(L, /^\s*pub use usage_script::.*$/);
  write('services\\mod.rs', L);

  const lib = read('lib.rs');
  dropRe(lib, /^\s*mod usage_script;$/);
  write('lib.rs', lib);

  const cmds = read('commands\\mod.rs');
  dropRe(cmds, /usage_script/);
  write('commands\\mod.rs', cmds);
}

// ── ProviderService: the two entry points that used the script runner ─────
{
  const L = read('services\\provider\\mod.rs');
  for (const [sig, body] of [
    ['pub async fn query_usage(', `    /// Usage lookup.
    ///
    /// Upstream ran a user-supplied JS snippet here. We do not: our balance
    /// endpoint is queried directly by \`services::balance\`, and shipping a JS
    /// eval for a single-provider tool would be a liability, not a feature.
    /// Script support is a phase-2 item.
    pub async fn query_usage(
        _state: &AppState,
        _app_type: AppType,
        _provider_id: &str,
    ) -> Result<UsageResult, AppError> {
        Err(AppError::Message(USAGE_SCRIPT_UNSUPPORTED.to_string()))
    }

    const USAGE_SCRIPT_UNSUPPORTED: &str = "该供应商未配置用量查询脚本";

    pub async fn test_usage_script(`],
    ['pub async fn test_usage_script(', null],
  ]) {
    if (!body) continue;
    const s = at(L, sig);
    if (s < 0) continue;
    let e = -1;
    for (let i = s + 1; i < L.length; i++) if (L[i] === '    }') { e = i; break; }
    if (e > s) rep(L, s, e, body);
  }
  write('services\\provider\\mod.rs', L);
}

console.log('done');