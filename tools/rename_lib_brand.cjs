// Remove upstream's product name from everything a user can see in lib.rs.
//
// A different set of strings than the tray's: startup banner, tray tooltip,
// migration failure dialogs and upgrade prompts. These are the ones that make
// the binary identify itself as somebody else's software — and the error
// dialogs in particular, which a user only ever sees when something has already
// gone wrong.
//
// Log messages are left alone: they are for someone reading a log to diagnose a
// fault, and "which build produced this" is a legitimate reason to read them.
// The MIT notice already requires the upstream provenance to stay legible.
'use strict';
const fs = require('fs');

const P = 'D:\\\\src\\\\cc-switch-main\\\\src-tauri\\\\src\\\\lib.rs';
let lines = fs.readFileSync(P, 'utf8').split(/\r?\n/);

let changed = 0;
const log = [];

for (let i = 0; i < lines.length; i++) {
  const line = lines[i];
  if (!line.includes('CC Switch')) continue;

  // Never touch a `//` comment, including a trailing one.
  const code = line.split('//')[0];
  if (!code.includes('CC Switch')) continue;

  // Only string literals, not identifiers.
  if (!/"/.test(code)) continue;

  const before = line;
  const out = code.replace(/CC Switch/g, 'Aimyria');
  if (out !== before) {
    lines[i] = out + line.slice(code.length);
    changed++;
    log.push(`  L${i + 1}  ${out.trim().slice(0, 76)}`);
  }
}

fs.writeFileSync(P, lines.join('\n'), 'utf8');
console.log(`changed ${changed} line(s)`);
for (const l of log) console.log(l);

const left = lines.filter(
  (l) => !l.trimStart().startsWith('//') && !l.trimStart().startsWith('*') && l.includes('CC Switch'),
);
console.log(`\nremaining user-visible mentions: ${left.length}`);
for (const r of left) console.log('  ! ' + r.trim().slice(0, 84));