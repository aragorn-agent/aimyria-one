// M3 phase 1 · batch 6: the last mechanically-fixable errors.
'use strict';
const fs = require('fs');
const R = 'D:\\src\\cc-switch-main\\src-tauri\\src\\';
const read = f => fs.readFileSync(R + f, 'utf8').split(/\r?\n/);
const write = (f, L) => { fs.writeFileSync(R + f, L.join('\n'), 'utf8'); console.log(`  ${f}: ${L.length}`); };
const at = (L, t, from = 0) => { for (let i = from; i < L.length; i++) if (L[i].includes(t)) return i; return -1; };
const rep = (L, a, b, text) => L.splice(a, b - a + 1, ...text.split('\n'));
const dropLines = (L, re) => { for (let i = L.length - 1; i >= 0; i--) if (re.test(L[i])) L.splice(i, 1); };

// ── app_config.rs: PromptConfig became self-referential when `prompt` went ──
{
  const L = read('app_config.rs');
  let s = at(L, '/// Prompt 配置：单客户端维度');
  let e = -1;
  for (let i = s; i < L.length; i++) if (L[i] === '}' && !L[i].startsWith('    ')) { e = i; break; }
  if (s >= 0 && e > s) L.splice(s, e - s + 2);          // struct + its closing brace
  write('app_config.rs', L);
}

// ── balance.rs: from_slice's error type is not String ────────────────────
{
  const L = read('services\\balance.rs');
  const s = at(L, 'let body: Value = serde_json::from_slice(&raw)');
  L[s + 1] = '        .map_err(|e| failure(&format!("unexpected response shape: {e}")))';
  L.splice(s + 2, 0, '        .unwrap_or(Value::Null);');
  // the trailing `?` turned the Ok(..) into an error path
  for (let i = s + 3; i < L.length; i++) if (L[i].trim() === '?;') { L[i] = ''; break; }
  write('services\\balance.rs', L);
}

// ── tray.rs ──────────────────────────────────────────────────────────────
{
  const L = read('tray.rs');
  // ProviderEntry gained two fields in the surviving struct
  let s = at(L, 'fn provider_entry(app: &AppType, provider: &Provider) -> ProviderEntry {');
  const e = L.findIndex((l, k) => k > s && l === '}');
  rep(L, s, e, `fn provider_entry(app: &AppType, provider: &Provider) -> ProviderEntry {
    ProviderEntry {
        id: provider.id.clone(),
        name: provider.name.clone(),
        hint: provider_hint(provider),
        needs_routing: provider_needs_routing(app, provider),
        official: provider.category.as_deref() == Some("official"),
        blocked_from_routing: false,
    }
}`);
  // quota_title takes (texts, snapshot); the caller only has the QuotaView
  const c = at(L, 'snapshot.quota.as_ref().and_then(quota_title)');
  if (c >= 0) L[c] = L[c].replace('snapshot.quota.as_ref().and_then(quota_title)', 'snapshot.quota.as_ref().and_then(|q| quota_title(texts, q))');
  // `Value` is not imported in this module
  const v = at(L, 'Provider::with_id(String::new(), String::new(), Value::Null, None)');
  if (v >= 0) L[v] = 'Provider::with_id(String::new(), String::new(), serde_json::Value::Null, None)';
  write('tray.rs', L);
}

// ── mod.rs ───────────────────────────────────────────────────────────────
{
  const L = read('services\\provider\\mod.rs');
  // the universal-provider impl block: the DAO was deleted with the feature
  let s = at(L, 'use crate::provider::UniversalProvider;');
  if (s >= 0) {
    let banner = s;
    while (banner > 0 && !L[banner].startsWith('// =====')) banner--;
    let last = L.length - 1;
    while (last > s && L[last].trim() === '') last--;
    L.splice(banner, last - banner + 1);
  }
  // claude_top_key_is_shared is a free fn; the callers use Self::
  for (let i = 0; i < L.length; i++) {
    if (L[i].includes('Self::claude_top_key_is_shared(')) L[i] = L[i].replace('Self::', '');
  }
  write('services\\provider\\mod.rs', L);
}

// ── usage_cache.rs: SubscriptionQuota is gone ────────────────────────────
{
  const L = read('services\\usage_cache.rs');
  dropLines(L, /^use crate::services::subscription/);
  write('services\\usage_cache.rs', L);
}

// ── usage_stats.rs ───────────────────────────────────────────────────────
{
  const L = read('services\\usage_stats.rs');
  dropLines(L, /^use crate::codex_config::claude_desktop_config/);
  write('services\\usage_stats.rs', L);
}

// ── settings.rs: s3_sync field accessors ─────────────────────────────────
{
  const L = read('settings.rs');
  let s = at(L, 'if let Some(s3) = &mut self.s3_sync {');
  if (s >= 0) {
    let e = L.findIndex((l, k) => k > s && l.trim() === '}');
    L.splice(s, e - s + 1);
  }
  write('settings.rs', L);
}

// ── provider.rs: AppType gained/lost variants ────────────────────────────
{
  const L = read('provider.rs');
  const s = at(L, 'AppType::Pi |');
  if (s >= 0 && !L[s].includes('Mcode')) L[s] = L[s].replace('AppType::Pi |', 'AppType::Pi | AppType::Mcode |');
  write('provider.rs', L);
}

console.log('done');