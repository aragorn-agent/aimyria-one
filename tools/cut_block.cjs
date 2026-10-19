// Delete Rust statement blocks by brace balance.
//
//   node cut_block.cjs <file> <startLine> [startLine...]
//
// Each start line must be the first line of a statement (e.g. `match x {`,
// `if cond {`, `// comment` directly above a statement is NOT included — pass
// the statement's first line and use `cut_block.cjs --with-comments` to also
// eat the doc/line comments immediately above it.
'use strict';
const fs = require('fs');

const withComments = process.argv.includes('--with-comments');
const args = process.argv.slice(2).filter(a => !a.startsWith('--'));
const file = args[0];
const starts = args.slice(1).map(Number).sort((a, b) => b - a);

const L = fs.readFileSync(file, 'utf8').split(/\r?\n/);

function clean(line) {
  let s = '', j = 0;
  while (j < line.length) {
    const c = line[j];
    if (c === '/' && line[j + 1] === '/') break;
    if (c === '/' && line[j + 1] === '*') { j += 2; while (j < line.length && !(line[j] === '*' && line[j + 1] === '/')) j++; j += 2; continue; }
    const raw = line.slice(j).match(/^(?:b?r|rb|br)(#*)"/);
    if (raw) { const close = '"' + raw[1]; const idx = line.indexOf(close, j + raw[0].length); j = idx >= 0 ? idx + close.length : line.length; s += 'RS'; continue; }
    if (c === '"') { j++; while (j < line.length) { if (line[j] === '\\') { j += 2; continue; } if (line[j] === '"') { j++; break; } j++; } s += '""'; continue; }
    if (c === "'") { const m = line.slice(j).match(/^'(\\.|[^\\'\n])'/); if (m) { s += "''"; j += m[0].length; continue; } s += "'"; j++; continue; }
    s += c; j++;
  }
  return s;
}
const C = L.map(clean);

for (const start of starts) {
  let s = start - 1;
  if (withComments) {
    while (s > 0 && (L[s - 1].trim().startsWith('//') || L[s - 1].trim() === '')) s--;
  }
  // Walk forward to the statement's end: brace balance, or a trailing `;`.
  let d = 0, seen = false, e = s;
  for (let k = s; k < L.length; k++) {
    for (const ch of C[k]) {
      if (ch === '{') { d++; seen = true; }
      else if (ch === '}') d--;
    }
    if (seen && d === 0) { e = k; break; }
    if (!seen && /;\s*$/.test(C[k])) { e = k; break; }
    if (k === s && !seen && /^\s*$/.test(C[k])) { e = k; break; }
  }
  console.log(`  L${s + 1}-L${e + 1} (${e - s + 1} ln)  "${(L[s] || '').trim().slice(0, 56)}"`);
  L.splice(s, e - s + 1);
}

fs.writeFileSync(file, L.join('\n'), 'utf8');
console.log(`\n${file.split('\\').pop()}: now ${L.length} lines`);