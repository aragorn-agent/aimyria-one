// commands/model_fetch.rs: the previous pass replaced the function head but
// matched the wrong closing brace (spawn_blocking's, not the function's), so
// `get_opencode_models` was left without a body or a closing brace.
'use strict';
const fs = require('fs');
const p = 'D:\\\\src\\\\cc-switch-main\\\\src-tauri\\\\src\\\\commands\\\\model_fetch.rs';

let L = fs.readFileSync(p, 'utf8').split(/\r?\n/);

const s = L.findIndex(l => l.includes('pub async fn get_opencode_models()'));
if (s < 0) { console.error('anchor not found'); process.exit(1); }
// The line after the head must be the start of the next item's doc comment.
if (!L[s + 2].startsWith('///')) {
  console.error('unexpected shape: L' + (s + 3) + ' = ' + JSON.stringify(L[s + 2]));
  process.exit(1);
}

L.splice(s + 1, 1, ...[
  '    // Upstream shelled out to `opencode models` to discover what the CLI has',
  '    // loaded. That path needs the OpenCode CLI locator, which was removed with',
  '    // the rest of the OpenCode support in M2. OpenCode config lands in phase 2;',
  '    // until then the honest answer is "not available", not a silent empty list',
  '    // that would read as "you have no models".',
  '    Err("OpenCode 支持将在后续版本提供".to_string())',
  '}',
]);

fs.writeFileSync(p, L.join('\n'), 'utf8');
console.log('model_fetch.rs -> ' + L.length + ' lines');