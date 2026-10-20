#!/usr/bin/env node
/**
 * Frontend/backend contract audit: does every invoke() the UI makes resolve to
 * a real #[tauri::command]?
 *
 * # Why this exists
 *
 * A dead command link fails at click time, in the webview console, as
 * "Command X not found" -- which is invisible to every test in this repo and to
 * anyone not watching devtools. It is the same shape of defect as the empty tray
 * handler: the code compiles, nothing fails, and a control simply does nothing
 * when used.
 *
 * # How it decides
 *
 * The backend's ground truth is the set of `fn` definitions carrying
 * `#[tauri::command]` (not the invoke_handler list -- a command can be defined
 * and wired in a second place, and matching the attribute is what actually
 * registers it). The frontend's claim is every `invoke("name")` string literal.
 * A frontend name with no such function is reported.
 *
 * Write-only when a level of diffs is wanted: exits non-zero on a dead link.
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const ROOT = process.argv[2] ?? join(import.meta.dirname, "..");

function walk(dir, ext, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) {
      if (name === "node_modules" || name === "target" || name === ".git") continue;
      walk(p, ext, out);
    } else if (ext.some((e) => name.endsWith(e))) {
      out.push(p);
    }
  }
  return out;
}

// Backend: #[tauri::command] and #[tauri::command(rename_all = "...")] alike.
// Anything between the attribute and `fn` (other attributes, doc comments,
// `pub`, `async`) is skipped, which is what makes both spellings match.
const commandNames = new Set();
for (const file of walk(join(ROOT, "src-tauri", "src"), [".rs"])) {
  const src = readFileSync(file, "utf8");
  const re = /#\[tauri::command[^\]]*\]([\s\S]{0,400}?)\bfn\s+([A-Za-z0-9_]+)/g;
  for (const m of src.matchAll(re)) commandNames.add(m[2]);
}

// Frontend: invoke("name") / invoke<Type>("name").
const used = new Map();
for (const file of walk(join(ROOT, "src"), [".ts", ".tsx"])) {
  const src = readFileSync(file, "utf8");
  const re = /invoke(?:<[^>]*>)?\(\s*["']([A-Za-z0-9_]+)["']/g;
  for (const m of src.matchAll(re)) {
    if (!used.has(m[1])) used.set(m[1], new Set());
    used.get(m[1]).add(file.replace(ROOT + "\\", "").replace(ROOT + "/", ""));
  }
}

const dead = [...used.keys()].filter((n) => !commandNames.has(n)).sort();

console.log("frontend calls (distinct): " + used.size);
console.log("backend commands:         " + commandNames.size);
console.log("");
if (dead.length === 0) {
  console.log("[PASS] every invoke() resolves to a #[tauri::command]");
  process.exit(0);
}
console.log(`[FAIL] ${dead.length} invoke(s) have no backend command:`);
for (const n of dead) {
  console.log("  " + n + "  <- " + [...used.get(n)].join(", "));
}
process.exit(1);
