// Replace the one read of a field this build does not have.
//
// `commands/config.rs` is upstream's file. It calls
// `state.proxy_service.is_running().await` on the Claude Desktop branch, and
// `AppState` here carries no proxy service — M2 removed it, because this build
// runs no local proxy.
//
// The value is passed to `claude_desktop_config::get_status`, which is a shim
// answering `supported: false` regardless (see src-tauri/src/claude_desktop_config.rs).
// So the read is pure plumbing: removing it changes no behaviour.
//
// Everything else in the file stays byte-identical to upstream. This is a deletion
// of a read, not new logic.

import { readFileSync, writeFileSync } from "node:fs";

const LIB = "D:/src/cc-switch-main/src-tauri/src/commands/config.rs";
const OLD = "state.proxy_service.is_running().await";

const text = readFileSync(LIB, "utf8");

const marker = `\n                ${OLD},`;
if (!text.includes(marker)) {
  // Fall back to any indentation, so the script does not silently do nothing.
  const any = text.includes(OLD);
  console.error(any ? "found the call but not with the expected indentation" : "call not found");
  process.exit(1);
}

const replacement = [
  "                // This build runs no local proxy, so there is no service to ask.",
  "                // The value goes to claude_desktop_config::get_status, which",
  "                // answers `supported: false` regardless — see",
  "                // src-tauri/src/claude_desktop_config.rs.",
  "                false",
].join("\n");

writeFileSync(LIB, text.replace(marker, `\n${replacement}`));
console.log("replaced the proxy_service read with `false`");
