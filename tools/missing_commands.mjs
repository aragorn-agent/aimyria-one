// List the commands the renderer calls that the backend does not define.
//
// # The bug this file was written to stop me making
//
// The first version matched `#[tauri::command]` immediately followed by `fn`.
// The second allowed doc comments. Both were wrong, and both were wrong in the
// same direction: reporting commands as missing that exist.
//
// Upstream writes all of these between the attribute and the signature:
//
//   #[tauri::command]
//   #[allow(deprecated)] // 兼容层命令，内部调用已废弃的 Service 方法
//   /// 设置启用状态并同步到客户端配置
//   pub async fn set_mcp_enabled(
//
// An attribute, a `//` comment, a `///` comment, and sometimes all three. The
// loose version reported 106 missing; the count came out 19 too high, and I was
// one step from telling the user our MCP screen had gone missing — all fifteen of
// those commands are defined and registered.
//
// Matching to the signature with a permissive gap is what works: attributes,
// comments of either kind, and blank lines, in any order, then the `fn`.

import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = "D:/src/cc-switch-main";
const SRC = join(ROOT, "src");
const RUST = join(ROOT, "src-tauri/src");

function walk(dir, filter) {
  const out = [];
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    if (statSync(p).isDirectory()) out.push(...walk(p, filter));
    else if (filter(p)) out.push(p);
  }
  return out;
}

const calls = new Map();
for (const file of walk(SRC, (p) => /\.tsx?$/.test(p))) {
  const text = readFileSync(file, "utf8");
  const rel = file.replace(SRC + "\\", "").replaceAll("\\", "/");
  for (const m of text.matchAll(/invoke(?:<[^>]*>)?\(\s*["']([a-z0-9_]+)["']/g)) {
    if (!calls.has(m[1])) calls.set(m[1], []);
    calls.get(m[1]).push(rel);
  }
}

// The gap between the attribute and the signature: attributes, comments of
// either kind, and blank lines, in any order.
const GAP = "(?:#\\[[^\\]]*\\]|//[^\\n]*|///[^\\n]*\\n|\\s)*";
const ATTR = new RegExp(
  "#\\[tauri::command(?:[^\\]]*)?\\]" + GAP + "\\s*(?:pub(?:\\([^)]*\\))?\\s+)?(?:async\\s+)?fn\\s+([a-z0-9_]+)",
  "g",
);

const defined = new Set();
const definedIn = new Map();
for (const file of walk(RUST, (p) => p.endsWith(".rs"))) {
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(ATTR)) {
    defined.add(m[1]);
    definedIn.set(m[1], file.replace(RUST + "\\", "").replaceAll("\\", "/"));
  }
}

const libText = readFileSync(join(RUST, "lib.rs"), "utf8");
const handlerStart = libText.indexOf("invoke_handler");
const handler = handlerStart === -1 ? "" : libText.slice(handlerStart);
const registered = new Set(
  [...handler.matchAll(/commands::(?:[a-z0-9_]+::)?([a-z0-9_]+)/g)].map((m) => m[1]),
);

const missing = [...calls.keys()].filter((c) => !defined.has(c)).sort();

// Buckets, because "106 missing" is not actionable and "35 of them need the local
// proxy" is.
const BUCKETS = [
  ["local proxy / routing / failover", /proxy|failover|circuit|route|stack|takeover|attach|exit_proxy|app_server|stale/],
  ["Copilot auth", /copilot/],
  ["managed auth (auth_*)", /^auth_/],
  ["profiles", /profile/],
  ["settings / tool management", /log_config|optimizer|rectifier|tool_version|installation|lifecycle|compression|disk_usage|multi_agent|pricing|tool_installations/],
  ["Pi / Hermes / Claude Desktop / OAuth", /pi_|hermes|claude_desktop|codex_oauth|xai|codex_session|codex_unified/],
  ["universal providers", /universal/],
];

const bucketed = new Set();
const lines = [];
for (const [label, re] of BUCKETS) {
  const hit = missing.filter((c) => re.test(c) && !bucketed.has(c));
  for (const c of hit) bucketed.add(c);
  if (hit.length) lines.push(`## ${label}  (${hit.length})`, "", ...hit.map((c) => `- ${c}`), "");
}
const rest = missing.filter((c) => !bucketed.has(c));
lines.push("## other", "", ...(rest.length ? rest.map((c) => `- ${c}`) : ["- (none)"]), "");

writeFileSync(join(ROOT, "tools/command-gaps.md"), [
  "# Frontend calls the backend does not answer",
  "",
  `frontend invokes **${calls.size}** commands; the backend defines **${defined.size}**.`,
  `**${missing.length}** of the invoked ones are not defined.`,
  "",
  ...lines,
].join("\n"));

console.log(`frontend invokes : ${calls.size}`);
console.log(`backend defines  : ${defined.size}`);
console.log(`missing          : ${missing.length}`);
console.log("");
console.log("tools/command-gaps.md");
for (const [label] of BUCKETS) {
  const n = missing.filter((c) => BUCKETS.find(([l, re]) => l === label)[1].test(c)).length;
  if (n) console.log(`  ${String(n).padStart(3)}  ${label}`);
}
if (rest.length) console.log(`  ${String(rest.length).padStart(3)}  other`);
