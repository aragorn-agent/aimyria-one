// Register every #[tauri::command] that is defined but never wired into the handler.
//
// # Why this exists
//
// 115 commands are defined in `src-tauri/src/commands/*` and are not in
// `invoke_handler`. Tauri only exposes what the handler lists, so from the
// renderer's side they do not exist: the call fails with
// "Command <name> not found".
//
// This is the third time this exact shape has cost the product a feature:
//
//   - `usage_stats.rs`, 5,150 lines, compiled for months while unreachable
//     because no command was registered (M19).
//   - `settings.rs` functions emptied by M2 leaving only their doc comments (M21).
//   - `grok_direct.rs` truncated mid-signature and never compiled, because the
//     module was never declared (M22).
//
// A command that is written but not listed is invisible to the compiler and
// invisible to the tests — `cargo check` and `cargo test` are both happy — and it
// only surfaces when a user clicks something. So the check has to be a script.
//
// Idempotent: a second run adds nothing.

import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = "D:/src/cc-switch-main";
const LIB = join(ROOT, "src-tauri/src/lib.rs");
const RUST = join(ROOT, "src-tauri/src");

function walk(dir) {
  const out = [];
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else if (p.endsWith(".rs")) out.push(p);
  }
  return out;
}

// `#[tauri::command]` may be followed by more attributes, `///` doc comments,
// `//` line comments or blank lines before the signature. Upstream uses all four,
// and a stricter pattern reported our own MCP commands as missing.
const ATTR =
  /#\[tauri::command(?:[^\]]*)?\](?:\s*|\n)*(?:#\[[^\]]*\]\s*|\/\/\/[^\n]*\n\s*|\/\/[^\n]*\n\s*)*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([a-z0-9_]+)/g;

const defined = new Set();
for (const file of walk(RUST)) {
  for (const m of readFileSync(file, "utf8").matchAll(ATTR)) defined.add(m[1]);
}

let lib = readFileSync(LIB, "utf8");

const handlerStart = lib.indexOf("invoke_handler");
if (handlerStart === -1) {
  console.error("no `invoke_handler` in lib.rs");
  process.exit(1);
}
const handlerEnd = lib.indexOf("]);", handlerStart);
if (handlerEnd === -1) {
  console.error("no closing `]);` for invoke_handler");
  process.exit(1);
}
const handler = lib.slice(handlerStart, handlerEnd);

const already = new Set(
  [...handler.matchAll(/commands::(?:[a-z0-9_]+::)?([a-z0-9_]+)/g)].map((m) => m[1]),
);

const missing = [...defined].filter((c) => !already.has(c)).sort();

if (missing.length === 0) {
  console.log("nothing to add — every defined command is registered");
  process.exit(0);
}

// Insert as a block, grouped, with the reason recorded in the file itself.
//
// Emitted as bare `commands::name` paths, not quoted strings:
// `tauri::generate_handler![commands::a, ...]` takes identifiers, and the first
// version of this script carried the names through as `"commands::a"` literals
// because they came out of a `match` already quoted. Rust stopped on line one
// with "expected identifier".
const block = [
  "",
  "            // ── Commands that were defined but never registered ─────────────",
  "            //",
  "            // Each of these compiled and passed `cargo test -test build` while",
  "            // being unreachable from the renderer, which answers",
  "            // `Command <name> not found`. 115 of them: the whole session manager,",
  "            // Hermes / OpenClaw / Omo / plugin / config / import-export / cloud",
  "            // sync / workspace surfaces, plus `stream_check_provider`, whose",
  "            // absence showed up as an error toast on the Codex provider page.",
  "            //",
  "            // See tools/register_commands.mjs.",
  ...missing.map((c) => `            commands::${c},`),
].join("\n");

const before = lib.slice(0, handlerEnd);
const after = lib.slice(handlerEnd);
lib = `${before}${block}${after}`;

writeFileSync(LIB, lib);
console.log(`registered ${missing.length} commands:`);
for (const c of missing) console.log(`  ${c}`);
