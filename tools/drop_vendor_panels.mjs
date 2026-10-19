// Delete the two vendor credential panels from UsageScriptModal, by line index.
//
// # Exact boundaries, read rather than inferred
//
//   Volcengine AK/SK : 1315-1404  (comment 1315, `)}` closes 1404)
//   Zhipu team       : 1406-1475  (comment 1406, `)}` closes 1475)
//
// Line numbers moved once already: the vendor-branch runtime code above the panels
// was simplified first, which shifted both panels up by 8 and 8 lines. The script
// verifies every boundary before splicing, so a stale range fails loudly instead
// of deleting the wrong 90 lines — which is what the first attempt did.
//
// The first attempt at this counted parentheses from the `&& (` and removed
// 90 lines too many, because the JSX between them contains nested `(` and `)`
// that belong to `t(...)` calls and ternaries rather than to the block. Backslash-
// escaped regexes and brace counting both failed here; what worked was reading the
// two endpoints off the screen.
//
// Higher range first, so the lower indices do not shift under the second splice.
// CRLF: splice by index, never match a `\n` anchor.
//
// # Why the panels go
//
// `CODING_PLAN_PROVIDERS` is empty, so `detectCodingPlanProvider` returns null for
// every URL, so nothing assigns `script.codingPlanProvider`. Both guards can only
// be false. They are unreachable UI carrying two competitor names, and they were
// the last thing holding the built-renderer brand rule red.

import { readFileSync, writeFileSync, copyFileSync } from "node:fs";

const FILE = "D:/src/cc-switch-main/src/components/UsageScriptModal.tsx";
const BACKUP = "C:/Users/a/AppData/Local/Temp/opencode/UsageScriptModal.panels.bak";

copyFileSync(FILE, BACKUP);

let lines = readFileSync(FILE, "utf8").split("\n");

// 1-based inclusive ranges.
const RANGES = [
  { name: "Zhipu team panel", from: 1406, to: 1475 },
  { name: "Volcengine AK/SK panel", from: 1315, to: 1404 },
];

// Confirm the boundaries before touching anything. A range that no longer lines up
// means the file moved, and deleting the wrong lines is worse than not deleting.
const expect = [
  { at: 1406, re: /智谱团队套餐/ },
  { at: 1408, re: /codingPlanProvider === "zhipu_team"/ },
  { at: 1475, re: /^\s*\)\}\s*$/ },
  { at: 1315, re: /火山方舟/ },
  { at: 1317, re: /codingPlanProvider === "volcengine"/ },
  { at: 1404, re: /^\s*\)\}\s*$/ },
];

for (const e of expect) {
  const line = lines[e.at - 1];
  if (!line || !e.re.test(line)) {
    console.error(
      `boundary check failed at line ${e.at}: expected /${e.re.source}/, got ${JSON.stringify(line)}`,
    );
    process.exit(1);
  }
}

for (const r of RANGES) {
  const count = r.to - r.from + 1;
  lines.splice(r.from - 1, count);
  console.log(`removed ${r.name}  (lines ${r.from}-${r.to}, ${count} lines)`);
}

writeFileSync(FILE, lines.join("\n"));

// Verify the result, since the failure mode of the first attempt was silent.
const after = readFileSync(FILE, "utf8").split("\n");
const leftovers = after.filter((l) => /codingPlanProvider === "(volcengine|zhipu_team)"/.test(l));
console.log(`\nfile now ${after.length} lines`);
console.log(`vendor id comparisons left: ${leftovers.length}`);
if (leftovers.length) {
  console.error("a vendor panel survived — restoring");
  copyFileSync(BACKUP, FILE);
  process.exit(1);
}
console.log(`backup: ${BACKUP}`);
