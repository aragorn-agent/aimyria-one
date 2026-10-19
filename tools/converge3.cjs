// Aimyria One · M2-b 收敛 v3：清理 unresolved 引用
// E0432/E0433/E0425/E0422/E0412 等「引用不存在项」——定位到对应 use 行并删除。
'use strict';
const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const SRC = 'D:\\src\\cc-switch-main\\src-tauri';
const ENV = {
  ...process.env,
  RUSTUP_HOME: 'D:\\Rust\\rustup', CARGO_HOME: 'D:\\Rust\\cargo',
  RUSTUP_TOOLCHAIN: '1.95.0-x86_64-pc-windows-gnu', CC: 'gcc', AR: 'ar',
  CARGO_TARGET_DIR: 'D:\\Rust\\build\\aimyria-one', CARGO_TERM_COLOR: 'never',
  PATH: 'D:\\Rust\\cargo\\bin;D:\\Tools\\mingw64\\mingw64\\bin;' + process.env.PATH,
};
const load = rel => fs.readFileSync(path.join(SRC, rel), 'utf8').split('\n');
const save = (rel, l) => fs.writeFileSync(path.join(SRC, rel), l.join('\n'), 'utf8');

function check() {
  try {
    execSync('cargo check --lib --message-format short', { cwd: SRC, env: ENV, stdio: 'pipe', timeout: 1800000 });
    return [];
  } catch (e) {
    return ((e.stdout || '') + (e.stderr || '')).split(/\r?\n/)
      .map(s => s.trim()).filter(s => /: error(\[|:)/.test(s));
  }
}

// 未解析引用类错误
const UNRESOLVED = /E0432|E0433|E0425|E0412|E0405|E0414|cannot find .* in .crate|unresolved import|could not find .* in the crate root|failed to resolve: use of (unresolved|undeclared)/i;

function useRange(lines, errLine) {
  // errLine 指向 use 语句（或其内部）。向上找 use/pub use 起点，向下按大括号平衡。
  let start = errLine;
  for (let k = errLine - 1; k >= 0 && k > errLine - 20; k--) {
    if (/^\s*(pub(\(crate\))?\s+)?use\s/.test(lines[k])) { start = k + 1; break; }
  }
  // 向下：若该 use 以 { 结尾未闭合，继续吃到配平
  let depth = 0, end = start - 1;
  for (let k = start - 1; k < lines.length && k < start + 60; k++) {
    const l = lines[k];
    for (const ch of l) { if (ch === '{') depth++; else if (ch === '}') depth--; }
    if (depth === 0 && (l.trim().endsWith(';') || l.trim().endsWith('}'))) { end = k + 1; break; }
    if (k + 1 > start + 50) break;
  }
  return [start, Math.max(end, start)];
}

const MAX = 15;
for (let round = 1; round <= MAX; round++) {
  const errs = check();
  if (!errs.length) { console.log('\n*** CLEAN ***'); process.exit(0); }
  const unresolved = errs.filter(s => UNRESOLVED.test(s));
  console.log(`\n--- round ${round}: ${errs.length} errors (${unresolved.length} unresolved) ---`);
  if (!unresolved.length) {
    console.log('remaining (non-unresolved):');
    errs.slice(0, 12).forEach(e => console.log('  ' + e));
    process.exit(1);
  }
  const byFile = new Map();
  for (const e of unresolved) {
    const m = e.match(/^(.+\.rs):(\d+)/);
    if (!m) continue;
    const rel = m[1].replace(/\\/g, '/');
    if (!byFile.has(rel)) byFile.set(rel, new Set());
    byFile.get(rel).add(+m[2]);
  }
  let n = 0;
  for (const [rel, set] of byFile) {
    const lines = load(rel);
    const ranges = [];
    for (const ln of [...set].sort((a, b) => a - b)) ranges.push(useRange(lines, ln));
    ranges.sort((a, b) => a[0] - b[0]);
    const merged = [];
    for (const r of ranges) {
      const last = merged[merged.length - 1];
      if (last && r[0] <= last[1] + 1) last[1] = Math.max(last[1], r[1]);
      else merged.push([...r]);
    }
    const drop = new Set();
    for (const [s, e] of merged) for (let i = s; i <= e; i++) drop.add(i);
    save(rel, lines.filter((_, i) => !drop.has(i + 1)));
    console.log(`  ${rel}: ${merged.length} use-groups removed (${lines.length} -> ${lines.length - drop.size})`);
    n += merged.length;
  }
  if (!n) { console.log('no change'); process.exit(1); }
}
console.log('\n*** MAX rounds ***');
