// Correct the paths of two commands the register script placed under `commands::`.
//
// `tools/register_commands.mjs` matched `#[tauri::command]` across the whole
// crate and emitted every hit as `commands::<name>`. Two of the 115 are not in
// `commands/`:
//
//   - `update_tray_menu` is defined in `lib.rs` and was already registered by
//     hand earlier in the same handler, so the script added a duplicate.
//   - `take_tray_navigation` lives in `tray.rs`, so the path is `crate::tray::`.
//
// Both showed up as `cannot find __cmd__<name> in commands`.

import { readFileSync, writeFileSync } from "node:fs";

const LIB = "D:/src/cc-switch-main/src-tauri/src/lib.rs";

let text = readFileSync(LIB, "utf8");
let changed = 0;

// `take_tray_navigation` → `crate::tray::take_tray_navigation`
{
  const re = /^\s*commands::take_tray_navigation,\s*$/m;
  if (re.test(text)) {
    text = text.replace(re, "            crate::tray::take_tray_navigation,");
    changed += 1;
  }
}

// Drop the duplicate `commands::update_tray_menu` — lib.rs's own is already listed.
{
  const re = /^\s*commands::update_tray_menu,\s*\r?\n/m;
  if (re.test(text)) {
    text = text.replace(re, "");
    changed += 1;
  }
}

if (changed === 0) {
  console.log("nothing to fix");
  process.exit(0);
}

writeFileSync(LIB, text);
console.log(`fixed ${changed} path(s)`);
