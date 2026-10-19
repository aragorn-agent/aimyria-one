// Add `tray_app_page_seen`, the last command the renderer calls that the backend
// does not define.
//
// Upstream lines 1299-1308 of tray.rs, verbatim. It clears an app's recorded
// switch failure once the user has opened that app's page — spec 5.5's "having seen
// the corresponding page" — and re-checks the tray so the problem dot goes away.
//
// Both helpers it needs already exist here with the same signatures
// (`clear_app_problems(&AppType) -> bool` at tray.rs:988,
// `schedule_tray_status_check(&AppHandle)` at tray.rs:1988), so this is the whole
// command and no supporting code.
//
// It was the only entry under Q1 in tools/command_surface.mjs — "renderer calls,
// backend does not define" — which is the question whose answer is a hard
// `Command X not found` at the call site.

import { readFileSync, writeFileSync } from "node:fs";

const TRAY = "D:/src/cc-switch-main/src-tauri/src/tray.rs";
const LIB = "D:/src/cc-switch-main/src-tauri/src/lib.rs";

let tray = readFileSync(TRAY, "utf8");

if (tray.includes("pub fn tray_app_page_seen")) {
  console.log("tray_app_page_seen already present");
} else {
  const anchor = "fn record_switch_failure(app: &AppType, reason: String) {";
  if (!tray.includes(anchor)) {
    console.error("anchor `record_switch_failure` not found");
    process.exit(1);
  }

  // Upstream lines 1299-1308, byte for byte.
  const cmd = [
    "",
    "/// 主界面打开了某个应用的页面：这个应用的问题行算处理过了（规格 5.5「打开过对应页面」）。",
    "#[tauri::command]",
    "pub fn tray_app_page_seen(app: tauri::AppHandle, app_type: String) {",
    "    let Ok(app_type) = app_type.parse::<AppType>() else {",
    "        return;",
    "    };",
    "    if clear_app_problems(&app_type) {",
    "        schedule_tray_status_check(&app);",
    "    }",
    "}",
    "",
  ].join("\n");

  tray = tray.replace(anchor, cmd + anchor);
  writeFileSync(TRAY, tray);
  console.log("added tray_app_page_seen to tray.rs");
}

let lib = readFileSync(LIB, "utf8");
if (/commands::tray_app_page_seen/.test(lib)) {
  console.log("already registered");
} else {
  const anchor = "            crate::tray::take_tray_navigation,";
  if (!lib.includes(anchor)) {
    console.error("anchor `crate::tray::take_tray_navigation,` not found");
    process.exit(1);
  }
  lib = lib.replace(anchor, `${anchor}\n            crate::tray::tray_app_page_seen,`);
  writeFileSync(LIB, lib);
  console.log("registered crate::tray::tray_app_page_seen");
}
