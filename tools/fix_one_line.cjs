// Only one line in the tree is genuinely damaged: services/provider/mod.rs:1090,
// where the GBK round-trip ate the closing quote of the usage-script message.
// Every other unbalanced-quote line reported by fix_quotes.cjs is a legitimate
// `\`-continued multi-line string literal, so it is left alone.
'use strict';
const fs = require('fs');
const p = 'D:\\\\src\\\\cc-switch-main\\\\src-tauri\\\\src\\\\services\\\\provider\\\\mod.rs';

let L = fs.readFileSync(p, 'utf8').split(/\r?\n/);
const i = 1089; // 0-based for line 1090

console.log('before: ' + JSON.stringify(L[i]));
if (!L[i].includes('Err(AppError::Message(')) {
  console.error('L1090 is not the expected line — aborting');
  process.exit(1);
}

L[i] = '        Err(AppError::Message("该供应商未配置用量查询脚本".to_string()))';
console.log('after:  ' + JSON.stringify(L[i]));

fs.writeFileSync(p, L.join('\n'), 'utf8');
console.log(p + ' -> ' + L.length + ' lines');