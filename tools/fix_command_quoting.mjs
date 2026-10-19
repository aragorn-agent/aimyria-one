// Fix the quoting on the block just inserted by tools/register_commands.mjs.
//
// `tauri::generate_handler![...]` takes command *paths* — `commands::product_info`
// — not strings. The script that inserted the block carried the names through as
// quoted literals, so the renderer got 115 entries like
// `"commands::apply_claude_onboarding_skip"` where an identifier was required, and
// `cargo check` stopped at the first one with "expected identifier".
//
// The neighbouring entries written by hand are unquoted, which is what made the
// mistake visible on one line: mine are the only quoted ones.

import { readFileSync, writeFileSync } from "node:fs";

const LIB = "D:/src/cc-switch-main/src-tauri/src/lib.rs";

const START_COMMENT = "// ── Commands that were defined but never registered";
const END_MARKER = "]);";

let text = readFileSync(LIB, "utf8");
const lines = text.split("\n");

const start = lines.findIndex((l) => l.includes(START_COMMENT));
if (start === -1) {
  console.error("inserted block not found — nothing to fix");
  process.exit(1);
}

// The block runs from the comment to the `]);` that closes generate_handler.
let end = -1;
for (let i = start; i < lines.length; i++) {
  if (lines[i].trim() === END_MARKER) {
    end = i;
    break;
  }
}
if (end === -1) {
  console.error("closing `]);` not found after the inserted block");
  process.exit(1);
}

let fixed = 0;
for (let i = start; i < end; i++) {
  const line = lines[i];
  const m = line.match(/^(\s*)"(commands::[a-z0-9_]+)"\s*,\s*$/);
  if (m) {
    lines[i] = `${m[1]}${m[2]},`;
    fixed += 1;
  }
}

if (fixed === 0) {
  console.log("no quoted entries left — already fixed");
  process.exit(0);
}

writeFileSync(LIB, lines.join("\n"));
console.log(`unquoted ${fixed} command paths`);
console.log(`  first: ${lines.find((l) => /^\s*commands::/.test(l) && l.includes("apply_claude_onboarding_skip"))?.trim()}`);
