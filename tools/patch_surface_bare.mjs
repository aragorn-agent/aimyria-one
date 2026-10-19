// Teach command_surface.mjs about commands lib.rs registers with no prefix.
//
// `update_tray_menu` is defined in lib.rs itself and listed in the handler as a
// bare `update_tray_menu,` — no `commands::` and no `crate::tray::`. The tool only
// recognised those two prefixed forms, so it reported lib.rs's own command as
// "defined but not registered", which is noise: that entry has been there and
// working since M16.
//
// The false positive mattered more than it looks. It was the only line under
// "Q2 defined, not registered", and Q2 is the question whose answer decides whether
// a feature is reachable — so a single bogus entry there costs the whole section
// its signal.

import { readFileSync, writeFileSync } from "node:fs";

const TOOL = "D:/src/cc-switch-main/tools/command_surface.mjs";

let text = readFileSync(TOOL, "utf8");
if (text.includes("Bare form")) {
  console.log("already patched");
  process.exit(0);
}

const anchor = "const registered = new Map(); // name -> how it is listed";
if (!text.includes(anchor)) {
  console.error("anchor not found");
  process.exit(1);
}

const add = [
  "// Bare form: lib.rs defines some commands itself (`update_tray_menu`) and lists",
  "// them with no prefix at all. Restricted to those two, so a stray identifier in",
  "// the handler cannot register itself.",
  "for (const bare of [\"update_tray_menu\", \"take_tray_navigation\"]) {",
  "  const re = new RegExp(\"^\\\\s*\" + bare + \",\\\\s*$\", \"m\");",
  "  if (re.test(handler)) registered.set(bare, bare);",
  "}",
  "",
].join("\n");

writeFileSync(TOOL, text.replace(anchor, `${add}${anchor}`));
console.log("patched bare-form detection");
