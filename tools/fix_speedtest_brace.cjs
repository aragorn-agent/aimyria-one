// speedtest.rs: `impl SpeedtestService {` was never closed — the file lost its
// final `}` in the M2 cuts. Verified with the brace tracker: final depth = 1.
'use strict';
const fs = require('fs');
const p = 'D:\\\\src\\\\cc-switch-main\\\\src-tauri\\\\src\\\\services\\\\speedtest.rs';

let L = fs.readFileSync(p, 'utf8').split(/\r?\n/);
while (L.length && L[L.length - 1].trim() === '') L.pop();

if (L[L.length - 1] !== '}') {
  L.push('}', '');
}

fs.writeFileSync(p, L.join('\n'), 'utf8');
console.log('speedtest.rs -> ' + L.length + ' lines, ends with: ' + JSON.stringify(L[L.length - 2]));