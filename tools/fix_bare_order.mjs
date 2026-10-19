// Move the bare-form block below the Map's declaration, by line index.
//
// Two attempts anchored on a string that was not there: the original loop reads
// `const name = ...; registered.set(name, m[1]);`, not the form the patch script
// assumed. Then a PowerShell-inline attempt died on backslash escaping — which is
// the fourth time in this project. Line-index splice and no interpolation.

import { readFileSync, writeFileSync } from "node:fs";

const TOOL = "D:/src/cc-switch-main/tools/command_surface.mjs";

let lines = readFileSync(TOOL, "utf8").split("\n");

// Find and remove the misplaced block: the comment line through its closing brace.
const start = lines.findIndex((l) => l.includes("// Bare form: lib.rs defines"));
if (start === -1) {
  console.log("no misplaced block found");
  process.exit(0);
}
let end = start;
while (end < lines.length && lines[end].trim() !== "}") end += 1;
lines.splice(start, end - start + 1);

// Re-insert after the line that closes the prefixed-form loop.
const after = lines.findIndex((l) => l.trim() === "registered.set(name, m[1]);");
if (after === -1) {
  console.error("could not find the prefixed-form loop body");
  process.exit(1);
}
const block = [
  "",
  "// Bare form: lib.rs defines some commands itself (update_tray_menu) and lists",
  "// them with no prefix at all. Restricted to those two, so a stray identifier in",
  "// the handler cannot register itself.",
  'for (const bare of ["update_tray_menu", "take_tray_navigation"]) {',
  '  const re = new RegExp("^\\\\s*" + bare + ",\\\\s*$", "m");',
  "  if (re.test(handler)) registered.set(bare, bare);",
  "}",
];
lines.splice(after + 1, 0, ...block);

writeFileSync(TOOL, lines.join("\n"));

// Verify the ordering, since the failure it fixes was silent.
const after2 = readFileSync(TOOL, "utf8");
const decl = after2.indexOf("const registered = new Map()");
const use = after2.indexOf("registered.set(bare");
if (decl === -1 || use === -1 || decl > use) {
  console.error("block still precedes the declaration");
  process.exit(1);
}
console.log("moved below the declaration");
