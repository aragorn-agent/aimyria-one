// Make one-click configuration always land in direct mode.
//
// # The defect
//
// A deep link left Codex's live config untouched. The log said:
//
//   [SWITCH] codex 代理模式：路…aimyria → aimyria
//   [SWITCH] codex 切到 aimyria 完成
//
// "切到完成" and no file written. Both are true: in routing mode
// `switch_inner` takes the proxy branch, which per its own comment
// "contract 没变时客户端文件不读也不写" — the client keeps pointing at the local
// proxy, and only the proxy's route changes.
//
// The mode was not wrong for upstream; it is wrong for this product. A user who
// tried 路由 once and then clicks 一键配置 expects five clients pointed at the
// relay, and gets one that silently is not.
//
// # The fix
//
// Before switching, put the app back to direct. `mode::controller::exit` is
// upstream's own function for leaving routing, and it is what the settings UI calls;
// this uses it at the point where a deep link promises "all five clients now use
// the relay".
//
// Only apps that are actually in a proxy mode are touched, so the common case is
// one cheap read.

import { readFileSync, writeFileSync } from "node:fs";

const APPLY = "D:/src/cc-switch-main/src-tauri/src/deeplink_apply.rs";

let text = readFileSync(APPLY, "utf8");

if (text.includes("force_direct_before_switch")) {
  console.log("already applied");
  process.exit(0);
}

// The helper goes just above the function that runs the per-client loop.
const anchor = "for spec in crate::live_writer::CLIENTS {";
if (!text.includes(anchor)) {
  console.error("anchor `for spec in crate::live_writer::CLIENTS {` not found");
  process.exit(1);
}

const helper = `/// Put an app back into direct mode before a deep link switches it.
///
/// A deep link's promise is that five clients now point at the relay. If the app is
/// in routing mode, \`ProviderService::switch\` takes the proxy branch instead and
/// deliberately does not touch the client's config file — the client keeps pointing
/// at the local proxy and only the proxy's route changes. The switch reports
/// success either way, so without this the link would appear to work while leaving
/// one client unrouted.
///
/// Called before the switch, once per app, only when the app is actually in a proxy
/// mode. \`mode::controller::exit\` is upstream's own path out of routing, the same
/// one the settings UI uses.
fn force_direct_before_switch(state: &crate::store::AppState, app_type: &crate::app_config::AppType) {
    if !crate::mode::current::is_proxy(app_type) {
        return;
    }
    // exit is async; block on it, as the rest of this file does for the same reason.
    if let Err(e) = futures::executor::block_on(crate::mode::controller::exit(state, app_type)) {
        // Not fatal: the switch below still runs, and a failure here is logged
        // rather than aborting the other four clients.
        log::warn!(
            "深链切换前退出 {} 的路由模式失败: {e}",
            app_type.as_str()
        );
    }
}

`;

text = text.replace(anchor, `${helper}${anchor}`);

// And call it at the top of the loop body, right after `app_type` is bound.
const callAnchor = "        let app_type = spec.app_type.clone();";
if (!text.includes(callAnchor)) {
  console.error("anchor for the loop-body app_type binding not found");
  process.exit(1);
}
text = text.replace(
  callAnchor,
  `${callAnchor}\n        force_direct_before_switch(state.inner(), &app_type);`,
);

writeFileSync(APPLY, text);
console.log("added force_direct_before_switch and its call site");
