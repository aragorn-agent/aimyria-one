// Precise Rust brace tracker: handles nested block comments, every raw-string
// form, byte strings, char literals vs lifetimes, and escapes.
// Prints depth whenever it is at or below `limit`, so you can see where an
// inner block failed to close.
'use strict';
const fs = require('fs');
const file = process.argv[2];
const limit = parseInt(process.argv[3] || '1', 10);
const src = fs.readFileSync(file, 'utf8');

let i = 0, line = 1, depth = 0, bcDepth = 0;
const out = [];
const RAW = /^(?:b?r|rb|br)(#*)"/;

function bump(ch) {
  if (ch === '\n') line++;
  else if (ch === '{') { depth++; }
  else if (ch === '}') { depth--; }
}

while (i < src.length) {
  const rest = src.slice(i, i + 12);
  if (bcDepth > 0) {
    if (src.startsWith('/*', i)) { bcDepth++; i += 2; continue; }
    if (src.startsWith('*/', i)) { bcDepth--; i += 2; continue; }
    if (src[i] === '\n') line++;
    i++;
    continue;
  }
  const raw = rest.match(RAW);
  if (raw) {
    const close = '"' + raw[1];
    // skip to end of the opening quote first
    let j = i + raw[0].length;
    while (j < src.length) {
      if (src[j] === '\n') line++;
      if (src.startsWith(close, j)) { j += close.length; break; }
      j++;
    }
    i = j;
    continue;
  }
  if (src.startsWith('//', i)) { while (i < src.length && src[i] !== '\n') i++; continue; }
  if (src.startsWith('/*', i)) { bcDepth = 1; i += 2; continue; }
  if (src[i] === 'b' && src[i + 1] === '"') {
    let j = i + 2;
    while (j < src.length) { if (src[j] === '\\') { j += 2; continue; } if (src[j] === '"') { j++; break; } if (src[j] === '\n') line++; j++; }
    i = j; continue;
  }
  if (src[i] === '"') {
    let j = i + 1;
    while (j < src.length) { if (src[j] === '\\') { j += 2; continue; } if (src[j] === '"') { j++; break; } if (src[j] === '\n') line++; j++; }
    i = j; continue;
  }
  if (src[i] === "'") {
    // char literal only when a closing quote follows within 8 chars and isn't a quote itself
    const m = rest.match(/^'(\\.|[^\\'\n])'/);
    if (m) { i += m[0].length; continue; }
    i++; continue;  // lifetime / label
  }
  bump(src[i]);
  if (depth <= limit) out.push(`${line}: depth=${depth}`);
  i++;
}

console.log(`final depth = ${depth}`);
const last = out[out.length - 1] || '(none)';
console.log(`last low-depth line: ${last}`);
console.log(`\nlines at depth<=${limit} (last 30):`);
for (const l of out.slice(-30)) console.log('  ' + l);