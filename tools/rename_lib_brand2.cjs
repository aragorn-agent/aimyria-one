const fs = require('fs');
const P = 'D:\\\\src\\\\cc-switch-main\\\\src-tauri\\\\src\\\\lib.rs';
let t = fs.readFileSync(P, 'utf8');

// These three live inside multi-line string literals (the `\` continuation
// style), so a line-oriented pass never saw them: it only sees the continuation
// line, which carries no quoting and reads as code.
const pairs = [
  ['建议回退到旧版本 CC Switch 以保护数据。', '建议回退到旧版本 Aimyria 以保护数据。'],
  ['Consider rolling back to an older CC Switch version.', 'Consider rolling back to an older Aimyria version.'],
  ['please upgrade CC Switch', 'please upgrade Aimyria'],
];
let n = 0;
for (const [from, to] of pairs) {
  if (t.includes(from)) { t = t.split(from).join(to); n++; }
  else { console.log('  MISS: ' + from.slice(0, 40)); }
}
fs.writeFileSync(P, t, 'utf8');
console.log('replaced ' + n + '/' + pairs.length);

const left = t.split(/\r?\n/).filter(
  (l) => !l.trimStart().startsWith('//') && !l.trimStart().startsWith('*') && l.includes('CC Switch'),
);
console.log('remaining user-visible mentions: ' + left.length);
for (const r of left) console.log('  ! ' + r.trim().slice(0, 84));