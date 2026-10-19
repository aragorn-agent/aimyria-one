// Map each call site to its enclosing top-level item in a Rust file.
// Usage: node enclosing.cjs <file.rs> <pattern-regex>
'use strict';
const fs = require('fs');

const file = process.argv[2];
const pat = new RegExp(process.argv[3]);
const lines = fs.readFileSync(file, 'utf8').split('\n');

// Strip strings / raw strings / char literals / comments for brace tracking.
function cleanLines(L) {
  const out = new Array(L.length);
  let inBc = false;
  for (let i = 0; i < L.length; i++) {
    const line = L[i];
    let s = '', j = 0;
    while (j < line.length) {
      const c = line[j];
      if (inBc) { if (c === '*' && line[j + 1] === '/') { inBc = false; j += 2; continue; } j++; continue; }
      if (c === '/' && line[j + 1] === '/') break;
      if (c === '/' && line[j + 1] === '*') { inBc = true; j += 2; continue; }
      // raw string: r"..." / r#"..."# / br#"..."#
      const raw = line.slice(j).match(/^(?:b?r)(#*)"/);
      if (raw) {
        const close = '"' + raw[1];
        const idx = line.indexOf(close, j + raw[0].length);
        j = idx >= 0 ? idx + close.length : line.length;
        s += 'RS'; continue;
      }
      if (c === '"') {
        j++;
        while (j < line.length) { if (line[j] === '\\') { j += 2; continue; } if (line[j] === '"') { j++; break; } j++; }
        s += '""'; continue;
      }
      if (c === "'") {
        const m = line.slice(j).match(/^'(\\.|[^\\'])'/);
        if (m) { s += "''"; j += m[0].length; continue; }
        s += "'"; j++; continue;
      }
      s += c; j++;
    }
    out[i] = s;
  }
  return out;
}

const cl = cleanLines(lines);

// Walk top-level items: record start line (after leading attrs/doc) and end line.
const items = [];
let attrs = [];
let i = 0;
while (i < lines.length) {
  const t = lines[i].trim();
  if (!t) { i++; continue; }
  if (t.startsWith('//') || t.startsWith('#[')) { attrs.push(i); i++; continue; }
  const start = attrs.length ? attrs[0] : i;
  let d = 0, seen = false, end = -1;
  for (let k = i; k < lines.length; k++) {
    for (const ch of cl[k]) { if (ch === '{') { d++; seen = true; } else if (ch === '}') d--; }
    if (seen && d === 0) { end = k; break; }
    if (!seen && cl[k].trim().endsWith(';')) { end = k; break; }
  }
  if (end < 0) end = lines.length - 1;
  items.push({ start: start + 1, sigLine: i + 1, end: end + 1, attrs, text: lines[i].trim() });
  attrs = [];
  i = end + 1;
}
console.log(`=== ${file}: ${items.length} top-level items ===`);

const hit = [];
for (let k = 0; k < lines.length; k++) {
  if (!pat.test(lines[k])) continue;
  const owner = items.find(it => k + 1 >= it.start && k + 1 <= it.end);
  hit.push({ line: k + 1, owner });
}
const byOwner = new Map();
for (const h of hit) {
  const key = h.owner ? `${h.owner.start}-${h.owner.end}` : 'NONE';
  if (!byOwner.has(key)) byOwner.set(key, { owner: h.owner, lines: [] });
  byOwner.get(key).lines.push(h.line);
}
for (const [key, v] of byOwner) {
  const o = v.owner;
  console.log(`\n  fn L${o.start}-L${o.end}  "${o.text.slice(0, 72)}"`);
  console.log(`     hits: ${v.lines.join(', ')}`);
}
console.log(`\nfinal depth check: ${(() => { let d = 0; for (const c of cl) for (const ch of c) { if (ch === '{') d++; else if (ch === '}') d--; } return d; })()}`);