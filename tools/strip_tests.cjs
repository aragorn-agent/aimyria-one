// Aimyria One · M2-c：删除全部测试
// 逐文件解析「顶层 item」（深度 0→1→0 的完整单元），
// 带 test/cfg(test) 属性的、或 mod xxx { ... } 形式的 #[cfg(test)] 块，统统删除。
'use strict';
const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const ROOT = 'D:\\src\\cc-switch-main\\src-tauri\\src';
const ENV = {
  ...process.env,
  RUSTUP_HOME: 'D:\\Rust\\rustup', CARGO_HOME: 'D:\\Rust\\cargo',
  RUSTUP_TOOLCHAIN: '1.95.0-x86_64-pc-windows-gnu', CC: 'gcc', AR: 'ar',
  CARGO_TARGET_DIR: 'D:\\Rust\\build\\aimyria-one', CARGO_TERM_COLOR: 'never',
  PATH: 'D:\\Rust\\cargo\\bin;D:\\Tools\\mingw64\\mingw64\\bin;' + process.env.PATH,
};

function walk(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p, out);
    else if (e.name.endsWith('.rs')) out.push(p);
  }
  return out;
}

// 计算每行的「净代码」：剥离字符串/字符字面量/行注释/块注释
function cleanLines(lines) {
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
      if (c === '"' || c === "'") {
        // Rust 生命周期 'a 与 char 字面量 'x'：仅当形如 'x' 且带闭合引号时视为字面量
        if (c === "'") {
          const m = line.slice(j).match(/^'(\\.|[^\\'])'/);
          if (m) { s += "''"; j += m[0].length; continue; }
          s += "'"; j++; continue;
        }
        j++; while (j < line.length) { if (line[j] === '\\') { j += 2; continue; } if (line[j] === '"') { j++; break; } j++; }
        s += '""'; continue;
      }
      s += c; j++;
    }
    out[i] = s;
  }
  return out;
}

// 找出所有顶层 item 区间 [start,end]（0-based, end 包含），以及其前置属性行区间
function topLevelItems(lines, clean) {
  const items = [];
  const attrs = [];   // 连续的属性/文档注释行
  let i = 0;
  const n = lines.length;
  while (i < n) {
    const t = lines[i].trim();
    if (!t) { i++; continue; }
    if (t.startsWith('//')) { attrs.push(i); i++; continue; }
    if (t.startsWith('#[')) { attrs.push(i); i++; continue; }
    // 可能是一个 item 的起始
    const start = i;
    let depth = 0, seen = false, end = -1;
    for (let k = i; k < n; k++) {
      const c = clean[k];
      for (const ch of c) { if (ch === '{') { depth++; seen = true; } else if (ch === '}') depth--; }
      if (seen && depth === 0) { end = k; break; }
      if (!seen && (c.trim().endsWith(';') || k > i)) {
        // 单行 item（如 const X: T = ...; 或 use x;）
        if (c.trim().endsWith(';')) { end = k; break; }
      }
      if (!seen && depth === 0 && k > i && !c.includes('(') && !c.includes('=')) { /* 继续 */ }
    }
    if (end < 0) { end = n - 1; }
    items.push({ start, end, attrFrom: attrs.length ? attrs[0] : start, attrs: attrs.slice() });
    attrs.length = 0;
    i = end + 1;
  }
  return items;
}

function stripTests(file, dry) {
  const raw = fs.readFileSync(file, 'utf8');
  const lines = raw.split('\n');
  const clean = cleanLines(lines);
  const items = topLevelItems(lines, clean);
  const drop = new Set();
  let removed = 0;
  for (const it of items) {
    const attrLines = lines.slice(it.attrFrom, it.start);
    const head = attrLines.join(' ') + ' ' + lines[it.start];
    const isTest = /#\[test\]|#\[tokio::test|#\[cfg\(test\)\]|#\[rstest\]/.test(head);
    // 独立的 #[cfg(test)] mod tests; 文件声明 -> 删文件由调用方处理
    if (!isTest) continue;
    for (let i = it.attrFrom; i <= it.end; i++) drop.add(i);
    removed += it.end - it.attrFrom + 1;
  }
  if (removed && !dry) {
    fs.writeFileSync(file, lines.filter((_, i) => !drop.has(i)).join('\n'), 'utf8');
  }
  return { removed, before: lines.length, after: lines.length - drop.size };
}

const dry = process.argv.includes('--dry');
let total = 0, files = 0;
const report = [];
for (const f of walk(ROOT)) {
  const base = path.basename(f);
  if (base === 'tests.rs') continue;           // 独立测试文件，整删
  const r = stripTests(f, dry);
  if (r.removed) { report.push(`  ${path.relative(ROOT, f).padEnd(44)} ${String(r.before).padStart(5)} -> ${String(r.after).padStart(5)}  (-${r.removed})`); total += r.removed; files++; }
}
console.log(report.join('\n'));
console.log(`\n${dry ? '[DRY] ' : ''}${files} files, ${total} lines of tests removed`);
