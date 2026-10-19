// M3 phase 1 · batch 14 — the last four.
'use strict';
const fs = require('fs');
const R = 'D:\\\\src\\\\cc-switch-main\\\\src-tauri\\\\src\\\\';

const read = f => fs.readFileSync(R + f, 'utf8').split(/\r?\n/);
const write = (f, L) => { fs.writeFileSync(R + f, L.join('\n'), 'utf8'); console.log('  ' + f + ': ' + L.length); };
const at = (L, t, from = 0) => { for (let i = from; i < L.length; i++) if (L[i].includes(t)) return i; return -1; };
const rep = (L, a, b, text) => L.splice(a, b - a + 1, ...text.split('\n'));
const edit = (f, fn) => { const L = read(f); fn(L); write(f, L); };

// ── tray.rs: the hoisted quota binding kept its old indent, its old
//    clone-rename, and a struct-literal trailing comma ───────────────────
edit('tray.rs', L => {
  const s = at(L, '            let quota = usage_view(');
  if (s < 0) { console.error('tray: quota block not found'); process.exit(1); }
  const blank = L.findIndex((l, k) => k > s && l.includes('snapshots.push(AppSnapshot {'));
  rep(L, s, blank - 1, ...[
    '        // Computed before the struct literal so `current_id` can still be',
    '        // borrowed here and moved in below.',
    '        let quota = usage_view(',
    '            &app_state.usage_cache,',
    '            texts,',
    '            app,',
    '            &current_provider,',
    '            current_id.as_deref().unwrap_or_default(),',
    '        );',
    '',
  ].join('\n').split('\n'));
});

// ── usage_stats.rs: the closure must yield an Option ───────────────────
edit('services\\\\usage_stats.rs', L => {
  const s = at(L, 'ModelPricing::from_strings(model_id, &input, &output, &cache_read, &cache_creation)');
  if (s >= 0) {
    L[s] = '            Some(ModelPricing::from_strings(model_id, &input, &output, &cache_read, &cache_creation))';
  }
});

// ── provider.rs: Pi is not one of our clients; say so explicitly ───────
edit('provider.rs', L => {
  if (at(L, 'AppType::Pi =>') >= 0) return;
  const anchor = at(L, '            // Claude and Claude Desktop both use the Anthropic-style env map');
  if (anchor < 0) { console.error('provider.rs anchor missing'); process.exit(1); }
  L.splice(anchor, 0, ...[
    '            // Pi is an upstream-only client. Aimyria One does not configure it,',
    '            // so there is nothing to read out of its row. Listed explicitly (not',
    '            // `_`) so adding a variant still forces a decision here.',
    '            AppType::Pi => (String::new(), String::new()),',
    '',
  ].join('\n').split('\n'));
});

// ── commands/model_fetch.rs: the OpenCode runtime-discovery body was cut
//    mid-chain. OpenCode lands in phase 2, so report that instead of
//    inventing a CLI invocation that may not exist. ──────────────────────
edit('commands\\\\model_fetch.rs', L => {
  const s = at(L, 'pub async fn get_opencode_models() -> Result<Vec<OpenCodeModelRef>, String> {');
  if (s < 0) { console.error('model_fetch anchor missing'); process.exit(1); }
  let e = -1;
  for (let i = s; i < L.length; i++) if (L[i] === '}') { e = i; break; }
  rep(L, s, e, ...[
    'pub async fn get_opencode_models() -> Result<Vec<OpenCodeModelRef>, String> {',
    '    // Upstream shelled out to `opencode models` to discover what the CLI has',
    '    // loaded. That path needs the OpenCode CLI locator, which was removed with',
    '    // the rest of the OpenCode support in M2. OpenCode config lands in phase 2;',
    '    // until then the honest answer is "not available", not a silent empty list',
    '    // that would read as "you have no models".',
    '    Err("OpenCode 支持将在后续版本提供".to_string())',
    '}',
  ].join('\n').split('\n'));
});

console.log('done');