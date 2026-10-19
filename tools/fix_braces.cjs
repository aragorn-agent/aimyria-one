// 修复括号失衡：扫描文件，删除导致深度变负的多余 '}'
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

const only = process.argv[2];
let total = 0;
for (const f of walk(ROOT)) {
  if (only && !f.endsWith(only)) continue;
  const lines = fs.readFileSync(f, 'utf8').split('\n');
  const cl = clean(lines);
  let d = 0, minD = 0;
  for (const c of cl) {
    for (const ch of c) { if (ch === '{') d++; else if (ch === '}') d--; }
    if (d < minD) minD = d;
  }
  if (d >= 0) continue;
  console.log(`${path.relative(ROOT, f)}  final=${d}`);
  if (d === 0) continue;
  // 只有当删除多余的顶层 } 后能归零时才自动修
  // 策略：反复删除「使深度恰好回到 0 后的第一个多余 } 」——改为逐行模拟，找到最终多余的 } 所在行
  let depth = 0;
  const depths = new Array(lines.length);
  for (let i = 0; i < lines.length; i++) {
    const b = depth;
    for (const ch of cl[i]) { if (ch === '{') depth++; else if (ch === '}') depth--; }
    depths[i] = depth;
  }
  // 从后往前找：删除该行后能减少最终深度的行（删 } 减少）
  const drop = new Set();
  let remaining = d;
  for (let i = lines.length - 1; i >= 0 && remaining < 0; i--) {
    const t = lines[i].trim();
    if (t !== '}' && t !== ');' && t !== '});' && t !== '},' && t !== '),') continue;
    // 删除此行会让最终深度 -1
    drop.add(i); remaining++;
    total++;
    console.log(`    drop L${i + 1}: ${t}`);
  }
  if (drop.size) {
    fs.writeFileSync(f, lines.filter((_, i) => !drop.has(i)).join('\n'), 'utf8');
  }
}
console.log(`\ntotal ${total} redundant closers flagged`);
