// Rename the user-visible tray strings from upstream's product name to ours.
//
// The tray is the only surface most users will ever see, so a menu that says
// "Quit CC Switch" identifies the tool as somebody else's software. This is a
// branding defect, not a cosmetic one.
//
// Scope is deliberately narrow: only strings the user can read are touched.
// Comments, log messages and doc comments keep referring to upstream, because
// those are for developers reading the source, and the MIT notice requires the
// upstream provenance to stay legible.
'use strict';
const fs = require('fs');

const P = 'D:\\\\src\\\\cc-switch-main\\\\src-tauri\\\\src\\\\tray.rs';
let lines = fs.readFileSync(P, 'utf8').split(/\r?\n/);

let changed = 0;
const log = [];

for (let i = 0; i < lines.length; i++) {
  const line = lines[i];

  // Only string-literal assignments (field initialisers in the i18n table) and
  // the tooltip fallback. Skip comments outright.
  const trimmed = line.trimStart();
  if (trimmed.startsWith('//') || trimmed.startsWith('///') || trimmed.startsWith('*')) continue;
  if (!line.includes('CC Switch')) continue;

  // Anything after a `//` on the same line is a trailing comment.
  const code = line.split('//')[0];
  if (!code.includes('CC Switch')) continue;

  // The routing service does not exist in this build, so the "stops the routing
  // service" variants describe something the user will never see happen.
  const before = line;
  let out = code
    .replace(/quit_stops_routing:\s*"([^"]*)"/g, (_m, _v) => 'quit_stops_routing: ""')
    .replace(/show_main:\s*"([^"]*)"/g, (m, v) => `show_main: "${v.replace(/CC Switch/g, 'Aimyria')}"`)
    .replace(/quit:\s*"([^"]*)"/g, (m, v) => `quit: "${v.replace(/CC Switch/g, 'Aimyria')}"`)
    .replace(/problem_more:\s*"([^"]*)"/g, (m, v) => `problem_more: "${v.replace(/CC Switch/g, 'Aimyria')}"`)
    .replace(/tooltip_problem:\s*"([^"]*)"/g, (m, v) => `tooltip_problem: "${v.replace(/CC Switch/g, 'Aimyria')}"`)
    .replace(/None\s*=>\s*"CC Switch"\.to_string\(\)/g, 'None => "Aimyria".to_string()');

  if (out !== before) {
    lines[i] = out + line.slice(code.length);
    changed++;
    log.push(`  L${i + 1}  ${out.trim().slice(0, 74)}`);
  }
}

fs.writeFileSync(P, lines.join('\n'), 'utf8');
console.log(`changed ${changed} line(s)`);
for (const l of log) console.log(l);

const remaining = lines.filter(
  (l) =>
    !l.trimStart().startsWith('//') &&
    !l.trimStart().startsWith('///') &&
    !l.trimStart().startsWith('*') &&
    l.includes('CC Switch'),
);
console.log(`\nremaining user-visible mentions: ${remaining.length}`);
for (const r of remaining.slice(0, 8)) console.log('  ! ' + r.trim().slice(0, 80));