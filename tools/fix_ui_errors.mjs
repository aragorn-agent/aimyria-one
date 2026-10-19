// Manage the three auth managers, insert the missing proxy command registration,
// and widen the window ACL.
//
// All three were found by clicking through the app:
//
//  - `Command set_proxy_takeover_for_app not found` — confirming 让 Codex 使用路由模式.
//  - `plugin:window|toggle_maximize not allowed by ACL` — the shell's title bar
//    maximize button, against a capabilities file written for a tray-only window
//    that lists show/hide/close/minimize/set-focus/start-dragging/is-visible and
//    nothing else.
//  - `state not managed for field copilotState on command stream_check_provider` —
//    CopilotAuthState / CodexOAuthState / XaiOAuthState are never `.manage()`d,
//    so every command taking one of them panics.
//
// The brand gate has a rule for that last class, "AppState is managed before
// anything reads it", and it only looks at AppState — three more unmanaged states
// passed straight under it. Worth widening that rule after this.

import { readFileSync, writeFileSync } from "node:fs";

const LIB = "D:/src/cc-switch-main/src-tauri/src/lib.rs";
const CAPS = "D:/src/cc-switch-main/src-tauri/capabilities/default.json";

let lib = readFileSync(LIB, "utf8");
const done = [];

// ── 1. window ACL ───────────────────────────────────────────────────────────
const caps = JSON.parse(readFileSync(CAPS, "utf8"));
const want = [
  "core:window:allow-toggle-maximize",
  "core:window:allow-set-fullscreen",
  "core:window:allow-is-fullscreen",
  "core:window:allow-is-maximized",
  "core:window:allow-set-size",
  "core:window:allow-set-position",
  "core:window:allow-get-all-windows",
  "core:window:allow-set-always-on-top",
];
let added = 0;
for (const p of want) if (!caps.permissions.includes(p)) (caps.permissions.push(p), added += 1);
writeFileSync(CAPS, `${JSON.stringify(caps, null, 2)}\n`);
done.push(`capabilities: +${added} window permissions`);

// ── 2. the missing proxy command ────────────────────────────────────────────
if (!lib.includes("commands::set_proxy_takeover_for_app")) {
  const anchor = "            commands::set_proxy_route,";
  if (!lib.includes(anchor)) {
    console.error("anchor `commands::set_proxy_route,` not found — run reregister_clean.mjs first");
    process.exit(1);
  }
  lib = lib.replace(anchor, `${anchor}\n            commands::set_proxy_takeover_for_app,`);
  done.push("registered set_proxy_takeover_for_app");
} else {
  done.push("set_proxy_takeover_for_app already registered");
}

// ── 3. manage the three auth managers ───────────────────────────────────────
if (!lib.includes("copilot_auth_manager")) {
  // Anchored on the SkillServiceState manage, which sits with the rest of setup.
  const anchor = /(\n\s*app\.manage\(commands::skill::SkillServiceState\([\s\S]*?\)\);\n)/;
  const m = lib.match(anchor);
  if (!m) {
    console.error("anchor for the SkillServiceState manage not found");
    process.exit(1);
  }
  const block = `

            // ── Auth managers, three of them ──────────────────────────────
            //
            // Copilot, Codex OAuth and xAI OAuth each hold a token store behind an
            // RwLock, and every command touching their tokens takes
            // State<'_, CopilotAuthState> and the like. Without these manage calls
            // Tauri panics with "state not managed for field copilotState", which
            // is what the Codex page's "default 检查出错" was.
            //
            // AppState already carries the Codex manager; these three wrap the
            // command-layer state types.
            {
                use crate::proxy::providers::copilot_auth::CopilotAuthManager;
                use crate::proxy::providers::xai_oauth_auth::XaiOAuthManager;
                use commands::{CodexOAuthState, CopilotAuthState, XaiOAuthState};
                use std::sync::RwLock;

                let app_config_dir = crate::config::get_app_config_dir();

                let copilot = CopilotAuthManager::new(app_config_dir.clone());
                app.manage(CopilotAuthState(std::sync::Arc::new(RwLock::new(copilot))));

                let codex_oauth = app.state::<AppState>().codex_oauth_manager.clone();
                app.manage(CodexOAuthState(codex_oauth));

                let xai = XaiOAuthManager::new(app_config_dir);
                app.manage(XaiOAuthState(std::sync::Arc::new(RwLock::new(xai))));

                log::info!("✓ Copilot / Codex OAuth / xAI OAuth managers initialized");
            }`;
  lib = lib.replace(anchor, `${m[1]}${block}\n`);
  done.push("managed CopilotAuthState / CodexOAuthState / XaiOAuthState");
} else {
  done.push("auth managers already managed");
}

writeFileSync(LIB, lib);
console.log(done.map((d) => `  ${d}`).join("\n"));
