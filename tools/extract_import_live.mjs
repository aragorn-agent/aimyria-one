// Extract two upstream functions verbatim into their own module.
//
// `import_hermes_providers_from_live` and `import_openclaw_providers_from_live`
// live in upstream's `services/provider/live.rs` (1,460 lines). That file opens
// with `use crate::proxy::providers::codex_oauth_auth::…` and has a
// `LiveSyncOutcome::ProxyMode` branch, so the whole file cannot be copied into a
// build with no proxy.
//
// The two import functions themselves never touch the proxy: each reads a
// client's own config file and upserts rows into the database. So they are lifted
// out as bytes — lines 1290-1460 of upstream's file, unmodified — into a module
// with only the imports they actually reference.
//
// This is a relocation, not a rewrite: if upstream's bodies change, re-running
// this overwrites them wholesale rather than drifting.

import { readFileSync, writeFileSync } from "node:fs";

const UP = "D:/src/cc-switch-upstream/src-tauri/src/services/provider/live.rs";
const OUT = "D:/src/cc-switch-main/src-tauri/src/services/provider/import_live.rs";

const up = readFileSync(UP, "utf8").split("\n");

// The four live-config entry points, contiguous, immediately before the test
// module. Boundaries read off the file rather than assumed: the first draft used
// `Measure-Object -Line` for the end, which reported 1460 for a 1663-line file
// and cut a function in half.
//
//   1290  pub fn import_openclaw_providers_from_live
//   1378  pub fn import_hermes_providers_from_live
//   1445  pub fn remove_hermes_provider_from_live
//   1464  pub fn remove_openclaw_provider_from_live
//   1479  #[cfg(test)]
const start = 1289;
const testAt = up.findIndex((l, i) => i > 1400 && l.trim() === "#[cfg(test)]");
if (testAt === -1) {
  console.error("could not find the #[cfg(test)] marker after the import functions");
  process.exit(1);
}
const end = testAt;
const body = up.slice(start, end).join("\n");

for (const fn of [
  "pub fn import_openclaw_providers_from_live",
  "pub fn import_hermes_providers_from_live",
  "pub fn remove_hermes_provider_from_live",
  "pub fn remove_openclaw_provider_from_live",
]) {
  if (!body.includes(fn)) {
    console.error(`expected ${fn} inside the extracted range`);
    process.exit(1);
  }
}

const header = `//! Live-config import for the additive-mode clients.
//!
//! Upstream's \`services/provider/live.rs\`, lines 1290-1460, extracted verbatim.
//! The rest of that file cannot be copied: it opens by importing
//! \`proxy::providers::codex_oauth_auth\` and carries a \`LiveSyncOutcome::ProxyMode\`
//! branch for provider takeover, neither of which means anything in a build that
//! points clients straight at the relay.
//!
//! These two functions are the exception — they read a client's own config file
//! and upsert rows, with no proxy in the path. Keeping them verbatim means the
//! import behaviour matches upstream exactly.

use crate::error::AppError;
use crate::hermes_config;
use crate::openclaw_config::{self, OpenClawProviderConfig};
use crate::provider::Provider;
use crate::store::AppState;

`;

writeFileSync(OUT, `${header}${body}\n`);
console.log(`wrote ${OUT}`);
console.log(`  ${body.split("\n").length} lines of upstream code`);
