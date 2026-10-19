#!/usr/bin/env node
/*
 * Aimyria One · M2-b 引用清理 · 块级切除
 *
 * 用法:
 *   node cut_blocks.js <file.rs> <errors.txt> [--apply]
 *
 * errors.txt 每行形如 "src/lib.rs:123"。
 * 把连续编译错误扩展到「完整语句 / 完整大括号块」边界后删除，避免语法残骸。
 * 默认 dry-run，只打印将删除的范围。
 */
'use strict';
const fs = require('fs');
const path = require('path');

const [, , fileArg, errArg, ...rest] = process.argv;
const apply = rest.includes('--apply');

// ── 1. 逐行剥离注释/字符串，计算大括号深度 ──
function profile(lines) {
  const before = new Int32Array(lines.length + 1);
  const after = new Int32Array(lines.length + 1);
  let d = 0, inBc = false;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    let out = '';
    let j = 0;
    while (j < line.length) {
      const c = line[j];
      if (inBc) {
        if (c === '*' && line[j + 1] === '/') { inBc = false; j += 2; continue; }
        j++; continue;
      }
      if (c === '/' && line[j + 1] === '/') break;
      if (c === '/' && line[j + 1] === '*') { inBc = true; j += 2; continue; }
      if (c === '"') {
        j++;
        while (j < line.length) {
          if (line[j] === '\\') { j += 2; continue; }
          if (line[j] === '"') { j++; break; }
          j++;
        }
        out += '""'; continue;
      }
      out += c; j++;
    }
    before[i + 1] = d;
    for (const ch of out) { if (ch === '{') d++; else if (ch === '}') d--; }
    after[i + 1] = d;
  }
  return { before, after, final: d };
}

// ── 2. 解析错误文件 ──
function parseErrors(txt) {
  const groups = new Map();
  for (const raw of txt.split(/\r?\n/)) {
    const t = raw.trim();
    if (!t) continue;
    const m = t.match(/^(.+\.rs):(\d+)$/);
    if (!m) continue;
    const f = m[1].replace(/\//g, '\\');
    if (!groups.has(f)) groups.set(f, new Set());
    groups.get(f).add(parseInt(m[2], 10));
  }
  return groups;
}

const lines = fs.readFileSync(fileArg, 'utf8').split('\n');
const { before, after } = profile(lines);
const groups = parseErrors(fs.readFileSync(errArg, 'utf8'));
const base = path.dirname(path.dirname(fileArg));   // ...\src-tauri

const cuts = [];
let totalLines = 0;

for (const [rel, set] of groups) {
  const full = path.join(base, rel);
  if (!fs.existsSync(full)) { console.log(`SKIP (not found): ${rel}`); continue; }
  const nums = [...set].sort((a, b) => a - b);

  // 归并成块（间隔 <= 8）
  const blocks = [];
  let st = nums[0], pv = nums[0];
  for (let k = 1; k < nums.length; k++) {
    if (nums[k] - pv > 8) { blocks.push([st, pv]); st = nums[k]; }
    pv = nums[k];
  }
  blocks.push([st, pv]);

  console.log(`\n=== ${rel} : ${nums.length} errors, ${blocks.length} blocks ===`);
  const own = fs.existsSync(full) && full === fileArg
    ? lines : fs.readFileSync(full, 'utf8').split('\n');
  const ownProf = full === fileArg ? { before, after }
    : profile(own);

  for (const [a, bRaw] of blocks) {
    const bEnd = Math.min(bRaw, own.length);
    const da = ownProf.before[a];
    const sTxt = own[a - 1].trim();
    let s, e;
    if (/^(use |[a-zA-Z_][\w:]*\s*[,;)]?\s*$)/.test(sTxt) && !sTxt.includes('{')) {
      s = a; e = a;
    } else {
      s = a;
      for (let i = a - 1; i >= 2; i--) {
        const pd = ownProf.before[i - 1];
        if (pd < da) break;
        if (pd === da && own[i - 2].trim() && !own[i - 2].trim().startsWith('//')) { s = i - 1; break; }
      }
      e = bEnd;
      const target = ownProf.before[s];
      for (let i = s; i <= own.length; i++) {
        if (i > bEnd && ownProf.after[i] === target) { e = i; break; }
      }
    }
    cuts.push({ full, s, e });
    totalLines += e - s + 1;
    let prev = own[s - 1].trim();
    if (prev.length > 76) prev = prev.slice(0, 76);
    console.log(`  cut L${s}-L${e} (${e - s + 1} lines)  ${prev}`);
  }
}

console.log(`\nTOTAL: ${cuts.length} ranges, ${totalLines} lines`);
if (!apply) { console.log('\n(dry-run - add --apply to write)'); process.exit(0); }

// ── 3. 从后往前应用 ──
const byFile = new Map();
for (const c of cuts) {
  if (!byFile.has(c.full)) byFile.set(c.full, []);
  byFile.get(c.full).push(c);
}
for (const [full, list] of byFile) {
  const L = fs.readFileSync(full, 'utf8').split('\n');
  const drop = new Set();
  for (const c of list) for (let i = c.s; i <= c.e; i++) drop.add(i);
  const kept = L.filter((_, i) => !drop.has(i + 1));
  fs.writeFileSync(full, kept.join('\n'), 'utf8');
  console.log(`APPLIED ${path.basename(full)}: ${L.length} -> ${kept.length} lines`);
}
