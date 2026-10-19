// 删除所有 顶层 #[test] / #[tokio::test] 函数（含其上方文档注释）
'use strict';
const fs = require('fs');
const path = require('path');
const ROOT = 'D:\\src\\cc-switch-main\\src-tauri\\src';

function clean(lines) {
  const out = new Array(lines.length);
  let inBc = false;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    let s = '', j = 0;
    while (j < line.length) {
      const c = line[j];
      if (inBc) { if (c === '*' && line[j + 1] === '/') { inBc = false; j += 2; continue; } j++; continue; }
      if (c === '/' && line[j + 1] === '/') break;
      if (c === '/' && line[j + 1] === '*') { inBc = true; j += 2; continue; }
      if (c === '"') { j++; while (j < line.length) { if (line[j] === '\\') { j += 2; continue; } if (line[j] === '"') { j++; break; } j++; } s += '""'; continue; }
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

function walk(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p, out); else if (e.name.endsWith('.rs')) out.push(p);
  }
  return out;
}

let totalFn = 0, totalLines = 0;
for (const f of walk(ROOT)) {
  const lines = fs.readFileSync(f, 'utf8').split('\n');
  const cl = clean(lines);
  const drop = new Set();

  for (let i = 0; i < lines.length; i++) {
    const t = lines[i].trim();
    if (!/^#\[(test|tokio::test|test_case)\]/.test(t)) continue;
    // 向上吃文档注释 / 空行 / 其他属性
    let s = i;
    for (let k = i - 1; k >= 0; k--) {
      const p = lines[k].trim();
      if (p === '' || p.startsWith('//') || p.startsWith('#[')) s = k; else break;
    }
    // 向下按大括号平衡（函数一定以 { 开头并闭合）
    let d = 0, seen = false, e = -1;
    for (let k = s; k < lines.length; k++) {
      for (const ch of cl[k]) { if (ch === '{') { d++; seen = true; } else if (ch === '}') d--; }
      if (seen && d === 0) { e = k; break; }
    }
    if (e < 0) e = Math.min(lines.length - 1, i + 40);
    for (let k = s; k <= e; k++) drop.add(k);
    totalFn++; totalLines += e - s + 1;
  }
  if (drop.size) {
    fs.writeFileSync(f, lines.filter((_, i) => !drop.has(i)).join('\n'), 'utf8');
    console.log(`  ${path.relative(ROOT, f).padEnd(42)} ${lines.length} -> ${lines.length - drop.size}`);
  }
}
console.log(`\nremoved ${totalFn} test fns, ${totalLines} lines`);
