// Manage the three auth managers the copied backend needs.
//
// `state not managed for field copilotState on command stream_check_provider` —
// CopilotAuthState, CodexOAuthState and XaiOAuthState are never `.manage()`d, so
// every command taking one of them panics. Found by clicking the 检查出错 toast on
// the Codex provider page.
//
// # Why this inserts by line number
//
// The previous two attempts anchored on a regex ending in `;\n`. lib.rs is CRLF —
// `;\r\n` — so `;\n` never matched and the script silently gave up having already
// written the capabilities file. Three separate edits in this project have now
// failed on line endings, so: find the line, splice by index, verify after.

import { readFileSync, writeFileSync } from "node:fs";

const LIB = "D:/src/cc-switch-main/src-tauri/src/lib.rs";

let text = readFileSync(LIB, "utf8");

if (text.includes("copilot_auth_manager")) {
  console.log("already managed");
  process.exit(0);
}

const lines = text.split("\n");

// The line that closes the SkillServiceState manage: `            )));`
const close = lines.findIndex(
  (l, i) => l.trim() === ")));" && lines[i - 1]?.includes("skill_service"),
);
if (close === -1) {
  console.error("could not find the SkillServiceState manage block");
  process.exit(1);
}

const block = [
  "",
  "            // ── Auth managers, three of them ──────────────────────────────",
  "            //",
  "            // Copilot, Codex OAuth and xAI OAuth each hold a token store behind",
  "            // an RwLock, and every command touching their tokens takes",
  "            // State<'_, CopilotAuthState> and the like. Without these manage",
  "            // calls Tauri panics with \"state not managed for field copilotState\",",
  "            // which is what the Codex page's \"default 检查出错\" was.",
  "            //",
  "            // AppState already carries the Codex manager; these three wrap the",
  "            // command-layer state types.",
  "            {",
  "                use crate::proxy::providers::copilot_auth::CopilotAuthManager;",
  "                use crate::proxy::providers::xai_oauth_auth::XaiOAuthManager;",
  "                use commands::{CodexOAuthState, CopilotAuthState, XaiOAuthState};",
  "                use std::sync::RwLock;",
  "",
  "                let app_config_dir = crate::config::get_app_config_dir();",
  "",
  "                let copilot = CopilotAuthManager::new(app_config_dir.clone());",
  "                app.manage(CopilotAuthState(std::sync::Arc::new(RwLock::new(copilot))));",
  "",
  "                let codex_oauth = app.state::<AppState>().codex_oauth_manager.clone();",
  "                app.manage(CodexOAuthState(codex_oauth));",
  "",
  "                let xai = XaiOAuthManager::new(app_config_dir);",
  "                app.manage(XaiOAuthState(std::sync::Arc::new(RwLock::new(xai))));",
  "",
  "                log::info!(\"✓ Copilot / Codex OAuth / xAI OAuth managers initialized\");",
  "            }",
];

lines.splice(close + 1, 0, ...block);
writeFileSync(LIB, lines.join("\n"));

// Verify, rather than trusting the splice.
const after = readFileSync(LIB, "utf8");
const names = [...after.matchAll(/commands::([a-z0-9_]+)/g)].map((m) => m[1]);
const dupes = [...new Set(names.filter((v, i) => names.indexOf(v) !== i))];
console.log("managed CopilotAuthState / CodexOAuthState / XaiOAuthState");
console.log(`  commands:: entries: ${names.length}, duplicates: ${dupes.length}`);
