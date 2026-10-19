// Classify each command the frontend calls but the backend does not define.
//
// The split that matters: a missing command is either copyable from upstream's
// Rust or it is gated behind the local routing proxy that M2 deleted. Copilot
// auth, proxy configuration and failover all sit behind it — copying their
// command signatures would compile and then do nothing, because they call into
// `crate::proxy`.
//
// The output is the list to copy, grouped by the upstream file it comes from.

import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = "D:/src/cc-switch-main";
const SRC = join(ROOT, "src");
const UP = "D:/src/cc-switch-upstream/src-tauri/src";

function walk(dir, filter) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) out.push(...walk(p, filter));
    else if (filter(p)) out.push(p);
  }
  return out;
}

// Frontend invokes.
const frontend = new Set();
for (const file of walk(SRC, (p) => /\.tsx?$/.test(p))) {
  const t = readFileSync(file, "utf8");
  for (const m of t.matchAll(/invoke(?:<[^>]*>)?\(\s*["']([a-z0-9_]+)["']/g)) {
    frontend.add(m[1]);
  }
}

// Our backend defines.
const ours = new Set();
for (const file of walk(join(ROOT, "src-tauri/src"), (p) => p.endsWith(".rs"))) {
  const t = readFileSync(file, "utf8");
  for (const m of t.matchAll(/#\[tauri::command[^\]]*\]\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([a-z0-9_]+)/g)) {
    ours.add(m[1]);
  }
}

const missing = [...frontend].filter((c) => !ours.has(c)).sort();

// Upstream: command name -> defining file, and whether that file needs the proxy.
const upFile = new Map();
for (const file of walk(UP, (p) => p.endsWith(".rs"))) {
  const t = readFileSync(file, "utf8");
  const rel = file.replace(UP + "\\", "").replaceAll("\\", "/");
  for (const m of t.matchAll(/#\[tauri::command[^\]]*\]\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([a-z0-9_]+)/g)) {
    if (!upFile.has(m[1])) upFile.set(m[1], rel);
  }
}

// A file is proxy-gated if it references crate::proxy or the proxy service.
const proxyGated = new Set();
for (const file of walk(UP, (p) => p.endsWith(".rs"))) {
  const t = readFileSync(file, "utf8");
  const rel = file.replace(UP + "\\", "").replaceAll("\\", "/");
  if (/crate::proxy|proxy_service|ProxyService/.test(t)) proxyGated.add(rel);
}

const byFile = new Map();
const copyable = [];
const gated = [];
const notFound = [];
for (const c of missing) {
  const f = upFile.get(c);
  if (!f) { notFound.push(c); continue; }
  if (proxyGated.has(f)) { gated.push({ cmd: c, file: f }); continue; }
  copyable.push({ cmd: c, file: f });
  if (!byFile.has(f)) byFile.set(f, []);
  byFile.get(f).push(c);
}

const lines = [
  "# 前端调用但后端缺失的命令分类",
  "",
  `缺失 ${missing.length} 个：可搬 **${copyable.length}**，依赖已删代理 **${gated.length}**，上游也找不到 **${notFound.length}**`,
  "",
  "## 可搬（按上游文件分组）",
  "",
];
for (const [f, cmds] of [...byFile.entries()].sort((a, b) => b[1].length - a[1].length)) {
  lines.push(`### \`${f}\`  (${cmds.length})`, "", ...cmds.map((c) => `- \`${c}\``), "");
}
lines.push("## 依赖已删的本地代理（搬了也是空壳）", "");
for (const g of gated) lines.push(`- \`${g.cmd}\`  ← ${g.file}`);
lines.push("", "## 上游也没找到", "", ...(notFound.length ? notFound.map((c) => `- \`${c}\``) : ["- （无）"]), "");

writeFileSync(join(ROOT, "tools/command-gaps.md"), lines.join("\n"));
console.log(`missing total     : ${missing.length}`);
console.log(`copyable          : ${copyable.length}`);
console.log(`proxy-gated       : ${gated.length}`);
console.log(`not found upstream: ${notFound.length}`);
console.log("");
console.log("按文件的可搬清单:");
for (const [f, cmds] of [...byFile.entries()].sort((a, b) => b[1].length - a[1].length)) {
  console.log(`  ${String(cmds.length).padStart(3)}  ${f}`);
}
