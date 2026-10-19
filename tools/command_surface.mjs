// Answer two questions that are easy to conflate, and were conflated.
//
// # The two questions
//
//   Q1. Does the backend define a command the renderer calls?
//       Failure mode: `Command <name> not found`, because there is no such fn.
//
//   Q2. Is every command the backend defines actually registered in
//       `tauri::generate_handler!`?
//       Failure mode: identical. `cargo check` and `cargo test` cannot see it —
//       removing an entry from the handler list is never an error.
//
// The old `tools/missing_commands.mjs` answered Q1 while being read as Q2. During
// M26 it reported "missing: 1" at the same time as 36 proxy commands sat defined
// and unregistered, and I reported that number to the user as though it were Q2's
// answer. It was not, and I was one step from telling them routing was wired up.
//
// # What this reports
//
// Both, separately, with the counts labelled. Q2 is the one that costs features.

import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = "D:/src/cc-switch-main";
const SRC = join(ROOT, "src");
const RUST = join(ROOT, "src-tauri/src");
const LIB = join(RUST, "lib.rs");

function walk(dir, filter) {
  const out = [];
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    if (statSync(p).isDirectory()) out.push(...walk(p, filter));
    else if (filter(p)) out.push(p);
  }
  return out;
}

// `#[tauri::command]` may be followed by more attributes, `///` doc comments,
// `//` line comments or blank lines before the signature. Upstream uses all four,
// and a stricter pattern reported our own MCP commands as missing.
const ATTR =
  /#\[tauri::command(?:[^\]]*)?\](?:\s*|\n)*(?:#\[[^\]]*\]\s*|\/\/\/[^\n]*\n\s*|\/\/[^\n]*\n\s*)*\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([a-z0-9_]+)/g;

const defined = new Map(); // name -> defining file
for (const file of walk(RUST, (p) => p.endsWith(".rs"))) {
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(ATTR)) {
    if (!defined.has(m[1])) {
      defined.set(m[1], file.replace(RUST + "\\", "").replaceAll("\\", "/"));
    }
  }
}

// Q2 — what the handler lists. Entries appear as `commands::name` or
// `commands::module::name` or, for the two defined outside `commands/`,
// `crate::tray::name`.
const lib = readFileSync(LIB, "utf8");
const handlerStart = lib.indexOf("invoke_handler");
if (handlerStart === -1) {
  console.error("no invoke_handler in lib.rs");
  process.exit(1);
}
const handler = lib.slice(handlerStart);
const registered = new Map(); // name -> how it is listed
for (const m of handler.matchAll(/\b(commands::(?:[a-z0-9_]+::)?([a-z0-9_]+)|crate::tray::([a-z0-9_]+))/g)) {
  const name = m[2] || m[3];
  registered.set(name, m[1]);

// Bare form: lib.rs defines some commands itself (update_tray_menu) and lists
// them with no prefix at all. Restricted to those two, so a stray identifier in
// the handler cannot register itself.
for (const bare of ["update_tray_menu", "take_tray_navigation"]) {
  const re = new RegExp("^\\s*" + bare + ",\\s*$", "m");
  if (re.test(handler)) registered.set(bare, bare);
}
}

// Q1 — what the renderer calls.
const calls = new Map();
for (const file of walk(SRC, (p) => /\.tsx?$/.test(p))) {
  const text = readFileSync(file, "utf8");
  const rel = file.replace(SRC + "\\", "").replaceAll("\\", "/");
  for (const m of text.matchAll(/invoke(?:<[^>]*>)?\(\s*["']([a-z0-9_]+)["']/g)) {
    if (!calls.has(m[1])) calls.set(m[1], []);
    calls.get(m[1]).push(rel);
  }
}

const calledButUndefined = [...calls.keys()].filter((c) => !defined.has(c)).sort();
const definedButUnregistered = [...defined.keys()]
  .filter((c) => !registered.has(c))
  .sort();
// Registered but not defined anywhere — would be a compile error, so this should
// be empty; kept as a tripwire.
const registeredButUndefined = [...registered.keys()]
  .filter((c) => !defined.has(c) && !["update_tray_menu", "take_tray_navigation"].includes(c))
  .sort();

const lines = [
  "# 命令面：两个问题，分开答",
  "",
  `- renderer 调用 **${calls.size}** 个命令`,
  `- 后端定义 **${defined.size}** 个`,
  `- invoke_handler 注册 **${registered.size}** 个`,
  "",
  `## Q1  前端调用但后端没定义 —— ${calledButUndefined.length}`,
  "",
  ...(calledButUndefined.length
    ? calledButUndefined.map((c) => `- \`${c}\` ← ${[...new Set(calls.get(c))].slice(0, 2).join(", ")}`)
    : ["- （无）"]),
  "",
  `## Q2  后端有定义但没注册 —— ${definedButUnregistered.length}`,
  "",
  ...(definedButUnregistered.length
    ? definedButUnregistered.map((c) => `- \`${c}\` ← ${defined.get(c)}`)
    : ["- （无）"]),
  "",
  `## 已注册但后端没定义 —— ${registeredButUndefined.length}`,
  "",
  ...(registeredButUndefined.length ? registeredButUndefined.map((c) => `- \`${c}\``) : ["- （无）"]),
  "",
];
writeFileSync(join(ROOT, "tools/command-surface.md"), lines.join("\n"));

console.log(`renderer calls          : ${calls.size}`);
console.log(`backend defines          : ${defined.size}`);
console.log(`handler registers        : ${registered.size}`);
console.log("");
console.log(`Q1 called, not defined   : ${calledButUndefined.length}`);
console.log(`Q2 defined, not registered: ${definedButUnregistered.length}   ← this one loses features`);
console.log(`registered, not defined  : ${registeredButUndefined.length}`);
console.log("");
console.log("tools/command-surface.md");

if (definedButUnregistered.length) {
  console.log("");
  console.log("Q2 detail (first 20):");
  for (const c of definedButUnregistered.slice(0, 20)) {
    console.log(`  ${c}  ← ${defined.get(c)}`);
  }
}
