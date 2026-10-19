// List items at a given brace depth (default 1 = items inside a top-level impl).
// Usage: node items_at_depth.cjs <file.rs> [depth]
'use strict';
const fs = require('fs');
const file = process.argv[2];
const target = parseInt(process.argv[3] || '1', 10);
const lines = fs.readFileSync(file, 'utf8').split('\n');

function cleanLines(L) {
  const out = new Array(L.length); let inBc = false;
  for (let i = 0; i < L.length; i++) {
    const line = L[i]; let s = '', j = 0;
    while (j < line.length) {
      const c = line[j];
      if (inBc) { if (c === '*' && line[j + 1] === '/') { inBc = false; j += 2; continue; } j++; continue; }
      if (c === '/' && line[j + 1] === '/') break;
      if (c === '/' && line[j + 1] === '*') { inBc = true; j += 2; continue; }
      const raw = line.slice(j).match(/^(?:b?r)(#*)"/);
      if (raw) { const close = '"' + raw[1]; const idx = line.indexOf(close, j + raw[0].length); j = idx >= 0 ? idx + close.length : line.length; s += 'RS'; continue; }
      if (c === '"') { j++; while (j < line.length) { if (line[j] === '\\') { j += 2; continue; } if (line[j] === '"') { j++; break; } j++; } s += '""'; continue; }
      if (c === "'") { const m = line.slice(j).match(/^'(\\.|[^\\'])'/); if (m) { s += "''"; j += m[0].length; continue; } s += "'"; j++; continue; }
      s += c; j++;
    }
    out[i] = s;
  }
  return out;
}
const cl = cleanLines(lines);

function walk(from, to, depth, prefix) {
  let i = from;
  while (i < to) {
    while (i < to && (!lines[i].trim() || lines[i].trim().startsWith('//'))) i++;
    if (i >= to) break;
    const attrsStart = i;
    while (i < to && lines[i].trim().startsWith('#[')) i++;
    let d = 0, seen = false, end = -1, sigEnd = -1;
    for (let k = i; k < to; k++) {
      const c = cl[k];
      let hasColon = false;
      for (const ch of c) {
        if (ch === '{') { d++; seen = true; }
        else if (ch === '}') d--;
        else if (ch === ':') hasColon = true;
      }
      if (seen && d === 0) { end = k; break; }
      if (!seen && hasColon && cl[k].trim().endsWith('{')) { /* keep scanning */ }
      if (!seen && /;\s*$/.test(c)) { end = k; break; }
    }
    if (end < 0) end = to - 1;
    const sig = lines.slice(i, Math.min(end + 1, i + 4)).map(s => s.trim()).filter(Boolean)[0] || '';
    console.log(`${prefix}L${attrsStart + 1}-L${end + 1}`.padEnd(24) + ` (${end - attrsStart + 1} ln)  ${sig.slice(0, 78)}`);
    if (seen) walk(i + 1, end, depth + 1, prefix + '  ');
    i = end + 1;
  }
}
walk(0, lines.length, 0, '');