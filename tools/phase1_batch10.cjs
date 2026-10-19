// M3 phase 1 · batch 10 — the last of it.
'use strict';
const fs = require('fs');
const R = 'D:\\src\\cc-switch-main\\src-tauri\\src\\';
const read = f => fs.readFileSync(R + f, 'utf8').split(/\r?\n/);
const write = (f, L) => { fs.writeFileSync(R + f, L.join('\n'), 'utf8'); console.log(`  ${f}: ${L.length}`); };
const at = (L, t, from = 0) => { for (let i = from; i < L.length; i++) if (L[i].includes(t)) return i; return -1; };
const rep = (L, a, b, text) => L.splice(a, b - a + 1, ...text.split('\n'));
const dropRe = (L, re) => { for (let i = L.length - 1; i >= 0; i--) if (re.test(L[i])) L.splice(i, 1); };

// ── commands/provider.rs: the stub must not name removed bindings ────────
{
  const L = read('commands\\provider.rs');
  const s = at(L, '    let _ = (app_type, state);');
  if (s >= 0) { L[s] = '    let _ = (app_type, settingsConfig, category, providerId, meta);'; }
  write('commands\\provider.rs', L);
}

// ── model_fetch.rs: use the timeout this function actually has ──────────
{
  const L = read('services\\model_fetch.rs');
  for (let i = 0; i < L.length; i++) {
    if (L[i].includes('from_secs(timeout_secs)')) {
      L[i] = L[i].replace('timeout_secs', 'self.timeout_secs');
    }
  }
  write('services\\model_fetch.rs', L);
}

// ── mod.rs: module-level constant (the impl is indented) ────────────────
{
  const L = read('services\\provider\\mod.rs');
  if (at(L, 'const USAGE_SCRIPT_UNSUPPORTED') < 0) {
    const i = L.findIndex(l => l.includes('impl ProviderService {'));
    if (i < 0) { console.error('impl anchor'); process.exit(1); }
    L.splice(i, 0,
      '/// Shown wherever the removed usage-script runner used to be called.',
      'const USAGE_SCRIPT_UNSUPPORTED: &str = "该供应商未配置用量查询脚本";',
      '');
  }
  write('services\\provider\\mod.rs', L);
}

// ── speedtest.rs: sanitize_timeout returns a Duration ──────────────────
{
  const L = read('services\\speedtest.rs');
  const s = at(L, 'let timeout = Self::sanitize_timeout(timeout_secs);');
  if (s >= 0) L[s] = '        let timeout = Self::sanitize_timeout(timeout_secs.unwrap_or(10));';
  const t = at(L, 'let (client, request_timeout) = Self::build_client(timeout)?;');
  if (t >= 0) L[t] = '        let (client, request_timeout) = Self::build_client(timeout.as_secs())?;';
  write('services\\speedtest.rs', L);
}

// ── stream_check.rs: build_result yields the value, not a Result ───────
{
  const L = read('services\\stream_check.rs');
  const s = at(L, 'Self::build_result(');
  if (s >= 0) {
    // `Ok(` on the previous line was already removed; make the tail match
    L[s + 1] = L[s + 1].replace('Ok(status)', 'Ok(status)');
  }
  const b = at(L, 'fn build_result(');
  if (b >= 0) {
    let e = -1;
    for (let i = b; i < L.length; i++) if (L[i] === '    }') { e = i; break; }
    rep(L, b, e, `fn build_result(
        result: Result<u16, AppError>,
        response_time: u64,
        degraded_threshold_ms: u64,
    ) -> StreamCheckResult {
        let tested_at = chrono::Utc::now().timestamp();
        match result {
            Ok(status) => StreamCheckResult {
                status: Self::determine_status(
                    response_time,
                    degraded_threshold_ms,
                ),
                success: status < 400,
                message: format!("HTTP {status}"),
                response_time_ms: Some(response_time),
                http_status: Some(status),
                tested_at,
                retry_count: 0,
            },
            Err(error) => StreamCheckResult {
                status: HealthStatus::Failed,
                success: false,
                message: error.to_string(),
                response_time_ms: Some(response_time),
                http_status: None,
                tested_at,
                retry_count: 0,
            },
        }
    }`);
  }
  write('services\\stream_check.rs', L);
}

// ── commands/usage.rs: session-usage sync is gone ──────────────────────
{
  const L = read('commands\\usage.rs');
  const s = at(L, 'pub fn get_session_usage_last_sync() -> Option<i64> {');
  if (s >= 0) {
    let e = -1;
    for (let i = s; i < L.length; i++) if (L[i] === '}') { e = i; break; }
    rep(L, s - 1, e, `pub fn get_session_usage_last_sync() -> Option<i64> {
    // Session-log scanning went away with the session manager. The command
    // stays so the frontend's poll keeps getting a well-formed answer.
    None
}`);
  }
  write('commands\\usage.rs', L);
}

// ── database/schema.rs ──────────────────────────────────────────────────
{
  const L = read('database\\schema.rs');
  const s = at(L, 'Self::migrate_v16_to_v17(');
  if (s >= 0) {
    let e = -1;
    let d = 0;
    for (let i = s; i < L.length; i++) {
      for (const ch of L[i]) { if (ch === '(') d++; else if (ch === ')') d--; }
      if (d <= 0) { e = i; break; }
    }
    L.splice(s, e - s + 1);
  }
  write('database\\schema.rs', L);
}

// ── backup_storage.rs: the Codex history-migration lock is gone ────────
{
  const L = read('services\\backup_storage.rs');
  let s = at(L, '    let _codex_guard = id');
  if (s >= 0) {
    let e = s;
    while (e < L.length && L[e].trim() !== '' && !L[e].trim().startsWith('let freed')) e++;
    L.splice(s, e - s);
  }
  dropRe(L, /^\s*whole\($/);
  dropRe(L, /codex_history_migration::/);
  dropRe(L, /DeviceStore::for_device\(\)/);
  write('services\\backup_storage.rs', L);
}

// ── editor_types.rs: give ConflictPolicy the resolver editor_toml calls ──
{
  const L = read('services\\provider\\editor_types.rs');
  if (at(L, 'pub fn resolve(') < 0) {
    const i = L.findIndex(l => l.includes('    KeepTheirs,'));
    if (i >= 0) {
      L.splice(i + 1, 0, '}',
        '',
        'impl ConflictPolicy {',
        '    /// Decide which changes may be written, given what is on disk now.',
        '    ///',
        '    /// A change conflicts when the live value differs from what the editor',
        '    /// started from *and* from what it would write — i.e. something else',
        '    /// changed that key while the editor was open.',
        '    pub fn resolve<T, F>(',
        '        &self,',
        '        changes: &[T],',
        '        mut conflict_of: F,',
        '    ) -> Result<(), Vec<String>>',
        '    where',
        '        F: FnMut(&T) -> Option<String>,',
        '    {',
        '        let conflicts: Vec<String> = changes.iter().filter_map(&mut conflict_of).collect();',
        '        match self {',
        '            ConflictPolicy::Refuse if !conflicts.is_empty() => Err(conflicts),',
        '            _ => Ok(()),',
        '        }',
        '    }',
        '}',
        '');
      // drop the enum\'s own closing brace, now that impl replaced it
      for (let k = i; k < L.length; k++) {
        if (k > i && L[k] === '}' && L[k + 1] === '') { L.splice(k, 1); break; }
      }
    }
  }
  write('services\\provider\\editor_types.rs', L);
}

console.log('done');