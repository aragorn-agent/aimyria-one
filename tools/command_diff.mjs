// Diff the command surface: what the frontend invokes vs what the backend defines.
//
// Two directions matter and they fail differently:
//
//   - Frontend calls a command the backend does not define: the screen throws at
//     runtime with "Command X not found". TypeScript does not catch it, because
//     `invoke` is untyped by design.
//   - The backend defines a command nobody calls: dead code, or the wiring step
//     that was missed (this project has been bitten by that twice — `usage_stats`
//     compiled for months and was unreachable).
//
// Run from the repo root. Writes a report and prints the summary.

import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = "D:/src/cc-switch-main";
const SRC = join(ROOT, "src");
const RUST = join(ROOT, "src-tauri/src");

function walk(dir, filter) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) out.push(...walk(p, filter));
    else if (filter(p)) out.push(p);
  }
  return out;
}

// ── Frontend: every `invoke("name"` / `invoke<...>("name"` literal ────────────
const frontend = new Map();
for (const file of walk(SRC, (p) => /\.tsx?$/.test(p))) {
  const text = readFileSync(file, "utf8");
  const rel = file.replace(SRC + "\\", "").replaceAll("\\", "/");
  for (const m of text.matchAll(/invoke(?:<[^>]*>)?\(\s*["']([a-z0-9_]+)["']/g)) {
    if (!frontend.has(m[1])) frontend.set(m[1], []);
    frontend.get(m[1]).push(rel);
  }
}

// ── Backend: every fn annotated #[tauri::command] ────────────────────────────
const defined = new Map();
for (const file of walk(RUST, (p) => p.endsWith(".rs"))) {
  const text = readFileSync(file, "utf8");
  const rel = file.replace(RUST + "\\", "").replaceAll("\\", "/");
  const re = /#\[tauri::command[^\]]*\]\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([a-z0-9_]+)/g;
  for (const m of text.matchAll(re)) {
    if (!defined.has(m[1])) defined.set(m[1], rel);
  }
}

// ── Backend: which of those are actually registered ─────────────────────────
const libText = readFileSync(join(RUST, "lib.rs"), "utf8");
const handlerStart = libText.indexOf("invoke_handler");
const handler = handlerStart === -1 ? "" : libText.slice(handlerStart, libText.indexOf("]);", handlerStart));
const registeredNames = new Set(
  [...handler.matchAll(/commands::([a-z0-9_]+)/g)].map((m) => m[1]),
);
// A command re-exported through a module (e.g. commands::skill::X) shows up
// under its module path; treat the trailing segment as the name too.
for (const m of handler.matchAll(/commands::([a-z0-9_]+)::([a-z0-9_]+)/g)) {
  registeredNames.add(m[2]);
}

const missing = [...frontend.keys()].filter((c) => !defined.has(c)).sort();
const unregistered = [...defined.keys()]
  .filter((c) => frontend.has(c) && !registeredNames.has(c))
  .sort();
const neverCalled = [...defined.keys()]
  .filter((c) => !frontend.has(c))
  .sort();

const report = [
  "# 命令面差集",
  "",
  `- 前端 invoke 了 **${frontend.size}** 个命令`,
  `- 后端用 #[tauri::command] 定义了 **${defined.size}** 个`,
  `- invoke_handler 注册了 **${registeredNames.size}** 个条目`,
  "",
  "## 前端调用但后端没定义（运行时会报 Command not found）",
  "",
  ...(missing.length ? missing.map((c) => `- \`${c}\`  ← ${frontend.get(c)[0]}`) : ["- （无）"]),
  "",
  "## 后端定义了但没注册（存在但调不到 —— 本项目已踩过两次）",
  "",
  ...(unregistered.length ? unregistered.map((c) => `- \`${c}\``) : ["- （无）"]),
  "",
  "## 后端定义了但前端没调（死代码，或前端用了别的路径）",
  "",
  ...(neverCalled.length ? neverCalled.map((c) => `- \`${c}\``) : ["- （无）"]),
  "",
].join("\n");

writeFileSync(join(ROOT, "tools/command-diff.md"), report);
console.log(`frontend invokes : ${frontend.size}`);
console.log(`backend defines  : ${defined.size}`);
console.log(`registered       : ${registeredNames.size}`);
console.log("");
console.log(`MISSING (frontend -> no backend): ${missing.length}`);
console.log(`DEFINED BUT NOT REGISTERED     : ${unregistered.length}`);
console.log(`DEFINED BUT NEVER CALLED       : ${neverCalled.length}`);
console.log("");
console.log("report: tools/command-diff.md");
