// Find and repair mojibake introduced by the PowerShell ANSI round-trip.
//
// Symptom: a string literal whose opening quote survives but whose *content*
// was re-encoded, so the closing quote turns into U+FFFD. The parser then runs
// the literal past the intended end of line and swallows real code, producing a
// cascade of nonsense "unknown prefix" errors far from the actual damage.
//
// U+FFFD (\\uFFFD) cannot appear in a legitimately UTF-8 encoded source file, so
// its presence is an unambiguous marker.
'use strict';
const fs = require('fs');
const path = require('path');

const ROOT = 'D:\\\\src\\\\cc-switch-main\\\\src-tauri\\\\src\\\\';

function walk(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p, out);
    else if (e.name.endsWith('.rs')) out.push(p);
  }
  return out;
}

const MSG = '该供应商未配置用量查询脚本';
let hits = 0;

for (const file of walk(ROOT)) {
  const L = fs.readFileSync(file, 'utf8').split(/\r?\n/);
  const dirty = [];
  for (let i = 0; i < L.length; i++) {
    if (L[i].indexOf('\uFFFD') >= 0) dirty.push(i);
  }
  if (!dirty.length) continue;

  console.log('\n' + path.basename(file) + ': ' + dirty.length + ' damaged line(s)');
  for (const i of dirty) {
    console.log('  L' + (i + 1) + ' ' + JSON.stringify(L[i].slice(0, 100)));

    // The only damage in this tree is the duplicated usage-script message:
    // an `Err(AppError::Message(...))` whose literal arg is garbled.
    if (/Err\(AppError::Message\(/.test(L[i])) {
      L[i] = L[i].replace(/"[\s\S*]*$/, '"' + MSG + '".to_string()))');
      console.log('    -> repaired to ' + JSON.stringify(L[i].trim()));
      hits++;
    } else {
      console.log('    !! NOT AUTO-REPAIRABLE — needs a human');
    }
  }
  fs.writeFileSync(file, L.join('\n'), 'utf8');
}

console.log('\nrepaired ' + hits + ' line(s)');