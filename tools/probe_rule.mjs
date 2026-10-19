// Import the brand rule's own `built renderer` check and run it, printing the
// internal `problems` array instead of the rendered output.
//
// # Why this exists
//
// I planted `"SubRouter is an AI relay aggregator…"` in a rendered i18n string,
// confirmed the bytes were in the bundle, and the rule still did not report it.
// A gate that misses a planted probe is worse than no gate: it prints FAIL for one
// brand and PASSES on the thing it was written for, which reads as verification.
//
// Reasoning about the code did not find it. Run its own logic and print.

import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { execFileSync } from "node:child_process";

const ROOT = "D:/src/cc-switch-main";
const DIST = join(ROOT, "dist");

// Load the rule module and pull out the check by name.
const src = readFileSync(join(ROOT, "tools/check_brand.mjs"), "utf8");
// The module runs on import, so strip the bottom-level invocation if any and eval
// the exported table.
const rulesMatch = src.match(/const RULES\s*=\s*\[[\s\S]*$/);
if (!rulesMatch) {
  console.error("could not find the RULES table");
  process.exit(1);
}

const tmp = join(ROOT, "tools/.rule-probe.mjs");
const body = `import { readFileSync, existsSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
const ROOT = ${JSON.stringify(ROOT)};
${rulesMatch[0]
  .replace(/^const RULES/, "export const RULES")
  .split("\n")
  .filter((l) => !/^for \(const rule of RULES/.test(l) && !/rule\.check\(\)/.test(l) && !/console\.log/.test(l))
  .join("\n")}
`;
writeFileSyncProbe(tmp, body);

try {
  const { RULES } = await import(`file://${tmp.replace(/\\/g, "/")}`);
  const rule = RULES.find((r) => /built renderer/.test(r.rule));
  if (!rule) {
    console.error("rule not found");
    process.exit(1);
  }
  const problems = rule.check();
  console.log(`problems: ${problems.length}`);
  for (const p of problems) console.log("  - " + p.slice(0, 160));
} finally {
  rmProbe(tmp);
}

function writeFileSyncProbe(p, s) {
  require("node:fs").writeFileSync(p, s);
}
function rmProbe(p) {
  require("node:fs").rmSync(p, { force: true });
}