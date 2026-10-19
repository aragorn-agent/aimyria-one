// tray.rs: the `quota` binding was hoisted out of the AppSnapshot literal but
// the hoisted `let quota = usage_view(...)` never made it back, leaving the
// struct literal missing its `quota` field. Restore both halves.
'use strict';
const fs = require('fs');
const p = 'D:\\\\src\\\\cc-switch-main\\\\src-tauri\\\\src\\\\tray.rs';

let L = fs.readFileSync(p, 'utf8').split(/\r?\n/);

const stray = L.findIndex(l => l.includes('Computed before the struct literal so `current_id` can still be'));
if (stray < 0) { console.error('anchor not found'); process.exit(1); }

L.splice(stray, 1, ...[
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
]);

const field = L.findIndex(l => l.trim() === 'stack_members: Vec::new(),');
if (field < 0) { console.error('stack_members field not found'); process.exit(1); }
L.splice(field + 1, 0, '            quota,');

fs.writeFileSync(p, L.join('\n'), 'utf8');
console.log('tray.rs -> ' + L.length + ' lines');