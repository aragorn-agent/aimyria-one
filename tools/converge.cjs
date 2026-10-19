// Aimyria One · M2-b 收敛循环 v2
// 一轮收集全部语法错误 -> 各自定位所属 item 边界 -> 合并去重后批量删除 -> 重新编译。
'use strict';
const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const SRC = 'D:\\src\\cc-switch-main\\src-tauri';
const ENV = {
  ...process.env,
  RUSTUP_HOME: 'D:\\Rust\\rustup',
  CARGO_HOME: 'D:\\Rust\\cargo',
  RUSTUP_TOOLCHAIN: '1.95.0-x86_64-pc-windows-gnu',
  CC: 'gcc', AR: 'ar',
  CARGO_TARGET_DIR: 'D:\\Rust\\build\\aimyria-one',
  CARGO_TERM_COLOR: 'never',
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

const SYNTAX = /unexpected closing delimiter|unclosed delimiter|mismatched closing|expected `;`|expected expression|expected item|non-item in item list|this doc comment|expected one of|expected identifier/i;

function itemRange(lines, errLine) {
  // 向上：找最近的顶层 item 起点（缩进 <= 4），并吃掉其上方的 /// // #[]
  let start = errLine;
  for (let k = errLine - 1; k >= 0 && k > errLine - 300; k--) {
    const raw = lines[k];
    if (!raw.trim()) continue;
    const ind = raw.length - raw.trimStart().length;
    if (ind > 4) continue;
    const t = raw.trim();
    if (/^(pub |pub\(crate\) )?(async )?(unsafe )?fn |^impl\b|^pub struct |^struct |^pub enum |^enum |^(pub )?const |^static |^type |^#\[tauri::command\]/.test(t)) {
      start = k;
      while (start > 0) {
        const p = lines[start - 1].trim();
        if (p.startsWith('///') || p.startsWith('//') || p.startsWith('#[') || p === '') start--;
        else break;
      }
      break;
    }
    // 顶层非缩进的语句（如 impl 内的 match 残片）也可作为起点
    if (ind === 0) { start = k; break; }
  }
  // 向下：按大括号 / 顶层分号取 item 结束
  let depth = 0, seenBrace = false, end = errLine;
  for (let k = start - 1; k < lines.length; k++) {
    const l = lines[k];
    for (const ch of l) { if (ch === '{') { depth++; seenBrace = true; } else if (ch === '}') depth--; }
    if (seenBrace && depth === 0) { end = k + 1; break; }
    if (!seenBrace) {
      const ind = l.length - l.trimStart().length;
      if (l.trim() && ind <= 4 && k + 1 > errLine && /;\s*$/.test(l)) { end = k + 1; break; }
    }
    if (k + 1 > errLine + 250) break;
  }
  return [start + 1, Math.max(end, errLine)];
}

const MAX = 10;
for (let round = 1; round <= MAX; round++) {
  const errs = check();
  if (!errs.length) { console.log(`\n*** CLEAN (converged) ***`); process.exit(0); }
  const syntax = errs.filter(s => SYNTAX.test(s));
  console.log(`\n--- round ${round}: ${errs.length} errors (${syntax.length} syntax) ---`);
  if (!syntax.length) { console.log('non-syntax leftovers:'); errs.slice(0, 8).forEach(e => console.log('  ' + e)); process.exit(1); }

  const byFile = new Map();
  for (const e of syntax) {
    const m = e.match(/^(.+\.rs):(\d+)/);
    if (!m) continue;
    const rel = m[1].replace(/\\/g, '/');
    if (!byFile.has(rel)) byFile.set(rel, new Set());
    byFile.get(rel).add(+m[2]);
  }

  let changed = 0;
  for (const [rel, lineSet] of byFile) {
    const lines = load(rel);
    const ranges = [];
    for (const ln of [...lineSet].sort((a, b) => a - b)) {
      const [s, e] = itemRange(lines, ln);
      ranges.push([s, e]);
      console.log(`  ${rel} cut L${s}-L${e}  "${lines[s - 1].trim().slice(0, 46)}"`);
    }
    // 合并重叠/相邻区间
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
    changed += merged.length;
  }
  if (!changed) { console.log('no change'); process.exit(1); }
}
console.log('\n*** reached MAX rounds ***');
