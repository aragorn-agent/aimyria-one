// Mojibake detection, take 2.
//
// The damage is UTF-8 bytes decoded as GBK: a two-byte GBK char swallowed the
// closing `"` of a string literal and left a `?` placeholder. The result is a
// line with an *odd* number of unescaped double quotes — which is exactly what
// makes the parser run the literal past end-of-line and swallow real code.
'use strict';
const fs = require('fs');
const path = require('path');

const ROOT = 'D:\\\\src\\\\cc-switch-main\\\\src-tauri\\\\src\\\\';
const MSG = '该供应商未配置用量查询脚本';

function walk(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p, out);
    else if (e.name.endsWith('.rs')) out.push(p);
  }
  return out;
}

// Count double quotes that are not escaped and not inside a char literal.
// Good enough for the damage profile here: the broken lines are all plain
// `Err(...)` calls with no lifetimes or raw strings nearby.
function quoteCount(line) {
  let n = 0;
  for (let i = 0; i < line.length; i++) {
    if (line[i] === '\\') { i++; continue; }
    if (line[i] === "'" && line[i + 1] !== '\\') {
      // skip char literals like 'x' so a quote inside one is not counted
      const m = /^'(\\.|[^'\\])'/.exec(line.slice(i));
      if (m) { i += m[0].length - 1; continue; }
    }
    if (line[i] === '"') n++;
  }
  return n;
}

const FIX = '--fix';
let repaired = 0;
let reported = 0;

for (const file of walk(ROOT)) {
  const L = fs.readFileSync(file, 'utf8').split(/\r?\n/);
  const dirty = [];
  for (let i = 0; i < L.length; i++) {
    const n = quoteCount(L[i]);
    // A line legitimately ends mid-literal only inside a multi-line string,
    // which this tree does not use. Odd count here means damage.
    if (n % 2 === 1 && !L[i].trim().startsWith('//')) dirty.push(i);
  }
  if (!dirty.length) continue;

  console.log('\n' + path.basename(file) + ': ' + dirty.length + ' unbalanced-quote line(s)');
  for (const i of dirty) {
    reported++;
    console.log('  L' + (i + 1) + ' ' + JSON.stringify(L[i].slice(0, 90)));
    if (process.argv.includes(FIX) && /Err\(AppError::Message\(/.test(L[i])) {
      L[i] = L[i].replace(/"[\s\S*]*$/, '"' + MSG + '".to_string()))');
      console.log('    -> ' + JSON.stringify(L[i].trim()));
      repaired++;
    }
  }
  if (process.argv.includes(FIX)) fs.writeFileSync(file, L.join('\n'), 'utf8');
}

console.log('\nreported ' + reported + ', repaired ' + repaired);