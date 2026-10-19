// Delete Rust statement blocks located by a content anchor rather than a line
// number, so several deletions in one file can't shift each other's targets.
//
//   node cut_by_text.cjs <file> <anchorText> [<anchorText>...]
//
// Each anchor matches the first line of the statement to remove. The block
// extends to the matching closing brace (or trailing `;`). Any contiguous
// comment lines and blank lines immediately above are removed too.
'use strict';
const fs = require('fs');

const file = process.argv[2];
const anchors = process.argv.slice(3);
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

let C = L.map(clean);
let removed = 0, missed = [];

for (const anchor of anchors) {
  // Re-scan C each time: earlier deletions invalidated it.
  C = L.map(clean);
  let s = -1;
  for (let i = 0; i < L.length; i++) if (L[i].includes(anchor)) { s = i; break; }
  if (s < 0) { missed.push(anchor); continue; }
  // Extend the start upward over the comment lines / blank lines that belong to
  // this statement, then step forward to the first real statement line: a
  // `//` line cleans to empty and would otherwise look like a blank terminator.
  let cs = s;
  while (cs > 0 && (L[cs - 1].trim().startsWith('//') || L[cs - 1].trim() === '')) cs--;
  while (s < cs && (L[s].trim().startsWith('//') || L[s].trim() === '')) s++;

  let d = 0, seen = false, e = s;
  for (let k = s; k < L.length; k++) {
    for (const ch of C[k]) { if (ch === '{') { d++; seen = true; } else if (ch === '}') d--; }
    if (seen && d === 0) { e = k; break; }
    if (!seen && /;\s*$/.test(L[k])) { e = k; break; }
    if (k === s && L[k].trim() === '') { e = k; break; }
  }
  console.log(`  L${s + 1}-L${e + 1} (${e - s + 1} ln)  "${(L[s] || '').trim().slice(0, 54)}"`);
  L.splice(s, e - s + 1);
  removed++;
}

fs.writeFileSync(file, L.join('\n'), 'utf8');
if (missed.length) console.log('  NOT FOUND: ' + missed.map(a => JSON.stringify(a)).join(', '));
console.log(`\n${file.split('\\').pop()}: removed ${removed}, now ${L.length} lines`);