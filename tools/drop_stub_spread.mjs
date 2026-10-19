// Remove the leftover quoted spread block from lib.rs.
//
// `tools/register_commands.mjs` used to emit the newly registered commands as
// `...["commands::a", "commands::b"],` — quoted strings inside a spread. Rust takes
// identifiers there, not strings, so the build stopped at line one; the names were
// then re-emitted one-per-line and the spread left behind as a stub.
//
// It is the only `...[` left in the file, and it carries no command that the
// unrolled block does not already carry (verified: 0 duplicate names), so removing
// it is deletion of dead text.

import { readFileSync, writeFileSync } from "node:fs";

const LIB = "D:/src/cc-switch-main/src-tauri/src/lib.rs";

let text = readFileSync(LIB, "utf8");

// The stub is `...[` ... `],]);` spanning one line in practice.
const before = text.length;
text = text.replace(/^\s*\.\.\.\[[^\]]*\],\]\);$/gm, "            ]);");
// Tidy: the line now reads `            ]);` — already the closing token.

// Count what went.
const removed = before - text.length;

if (removed === 0) {
  console.log("no quoted spread found");
  process.exit(0);
}

writeFileSync(LIB, text);
console.log(`removed the quoted spread (${removed} chars)`);

// Verify no command now appears twice.
const all = [...text.matchAll(/commands::([a-z0-9_]+)/g)].map((m) => m[1]);
const dupes = [...new Set(all.filter((v, i) => all.indexOf(v) !== i))];
console.log(`commands:: entries: ${all.length}, duplicates: ${dupes.length}`);
if (dupes.length) {
  console.error("duplicate command names: " + dupes.join(", "));
  process.exit(1);
}
