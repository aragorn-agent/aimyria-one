// Restore the startup-import guard that upstream has and the M2 trim dropped.
//
// # The defect
//
// Two provider rows pointed at the same relay: `Aimyria`, created by the deep
// link, and `default`, created at startup. `default` was also the one marked
// 使用中 — so the user's active provider is the one they did not choose, and the
// list reads as a menu of alternatives when there is one relay.
//
// # Why
//
// Upstream's startup loop skips an app unless
// `should_import_default_config_on_startup` says so. The M2 trim kept the
// `import_default_config` call and lost the guard in front of it, so every
// non-additive app re-imported whatever was already in its live config — which,
// after a deep link had configured it, is our own row, imported again under the
// name "default".
//
// A visible competitor row one click from our own is exactly what the brand gate's
// "No competitor presets are seeded" is about; a duplicate of *ours* is the same
// surface with a different label.
//
// Insert before the `match import_default_config` line, by line index, because
// lib.rs is CRLF and `\n` anchors have failed silently three times in this file.

import { readFileSync, writeFileSync } from "node:fs";

const LIB = "D:/src/cc-switch-main/src-tauri/src/lib.rs";

const text = readFileSync(LIB, "utf8");

if (text.includes("should_import_default_config_on_startup")) {
  console.log("guard already present");
  process.exit(0);
}

const lines = text.split("\n");
const at = lines.findIndex((l) =>
  /ProviderService::import_default_config\(/.test(l),
);
if (at === -1) {
  console.error("could not find the import_default_config call");
  process.exit(1);
}

// The `match` keyword is on the line before.
const matchAt = at;

const guard = [
  "                // Skip apps whose live config should not become a provider row.",
  "                //",
  "                // Upstream guards this import; the M2 trim kept the call and lost",
  "                // the guard, so every non-additive app re-imported its own live",
  "                // config after a deep link had configured it — producing a second",
  "                // row named \"default\" next to the Aimyria row the link created,",
  "                // and marking the duplicate as 使用中.",
  "                if !crate::services::provider::should_import_default_config_on_startup(",
  "                    &app_state,",
  "                    &app_type,",
  "                )",
  "                .unwrap_or(false)",
  "                {",
  "                    log::debug!(",
  "                        \"○ {} live import skipped\",",
  "                        app_type.as_str()",
  "                    );",
  "                    continue;",
  "                }",
  "",
];

lines.splice(matchAt, 0, ...guard);
writeFileSync(LIB, lines.join("\n"));

// Verify: the guard is present, and the match call follows it.
const after = readFileSync(LIB, "utf8");
const guardAt = after.indexOf("should_import_default_config_on_startup");
const matchAt2 = after.indexOf("ProviderService::import_default_config");
if (guardAt === -1 || guardAt > matchAt2) {
  console.error("guard did not land before the import call");
  process.exit(1);
}
console.log("restored should_import_default_config_on_startup guard");
