// Aimyria One · M2-b 收敛 v4
// 处理两类：(A) unresolved import -> 删 use 行
//           (B) cannot find function/struct -> 删调用所在语句（含外层表达式）
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

const A = /E0432|E0433|E0425|E0412|E0405|E0414|unresolved import|could not find .* in the crate root|use of unresolved module/i;
const B = /E0425|E0422|E0412|cannot find function|cannot find (value|tuple struct|struct|macro|enum|constant)/i;

function useRange(lines, ln) {
  let s = ln;
  for (let k = ln - 1; k >= 0 && k > ln - 20; k--) if (/^\s*(pub(\(crate\))?\s+)?use\s/.test(lines[k])) { s = k + 1; break; }
  let d = 0, e = s - 1;
  for (let k = s - 1; k < lines.length && k < s + 80; k++) {
    for (const ch of lines[k]) { if (ch === '{') d++; else if (ch === '}') d--; }
    if (d === 0 && (lines[k].trim().endsWith(';') || lines[k].trim().endsWith('}'))) { e = k + 1; break; }
    if (k + 1 > s + 70) break;
  }
  return [s, Math.max(e, s)];
}

// 删除「包含该调用的整条语句」：从语句起点到语句结束（括号+分号平衡）
function stmtRange(lines, ln) {
  // 向上：找到语句起始（可能是多行链式调用）
  let s = ln;
  for (let k = ln - 1; k >= 0 && k > ln - 25; k--) {
    const raw = lines[k];
    if (!raw.trim()) continue;
    const ind = raw.length - raw.trimStart().length;
    const prev = k > 0 ? lines[k - 1] : '';
    // 上一行若以 ; { } 结束，或缩进更小，则当前行是语句起点
    if (/[;{}]\s*$/.test(prev.trim()) || ind === 0 || prev.trim() === '') { s = k + 1; break; }
    if (/;\s*$/.test(raw)) break;
  }
  // 向下：从 s-1 起，按 () {} [] 与分号找语句结束
  let par = 0, brc = 0, brk = 0, e = s - 1;
  for (let k = s - 1; k < lines.length && k < s + 60; k++) {
    const l = lines[k];
    const code = l.replace(/"(?:[^"\\]|\\.)*"/g, '""');
    for (const ch of code) {
      if (ch === '(') par++; else if (ch === ')') par--;
      else if (ch === '{') brc++; else if (ch === '}') brc--;
      else if (ch === '[') brk++; else if (ch === ']') brk--;
    }
    if (par <= 0 && brc <= 0 && brk <= 0 && /;\s*$/.test(l)) { e = k + 1; break; }
    if (k + 1 > s + 50) break;
  }
  return [s, Math.max(e, s)];
}

const MAX = 20;
for (let round = 1; round <= MAX; round++) {
  const errs = check();
  if (!errs.length) { console.log('\n*** CLEAN ***'); process.exit(0); }
  const byFile = new Map();
  let classified = 0;
  for (const e of errs) {
    const m = e.match(/^(.+\.rs):(\d+)/);
    if (!m) continue;
    let kind = null;
    if (A.test(e)) kind = 'use';
    else if (B.test(e)) kind = 'stmt';
    if (!kind) continue;
    classified++;
    const rel = m[1].replace(/\\/g, '/');
    if (!byFile.has(rel)) byFile.set(rel, []);
    byFile.get(rel).push({ line: +m[2], kind });
  }
  console.log(`\n--- round ${round}: ${errs.length} errors, ${classified} actionable ---`);
  if (!classified) {
    console.log('unhandled:'); errs.slice(0, 10).forEach(e => console.log('  ' + e));
    process.exit(1);
  }
  for (const [rel, items] of byFile) {
    const lines = load(rel);
    const ranges = items.map(it => (it.kind === 'use' ? useRange : stmtRange)(lines, it.line));
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
    console.log(`  ${rel}: ${merged.length} blocks removed (${lines.length} -> ${lines.length - drop.size})`);
  }
}
console.log('\n*** MAX rounds ***');
