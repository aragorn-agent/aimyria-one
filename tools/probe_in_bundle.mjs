// Is the probe text actually in the bundle?
//
// The rule did not report it, and there are two possibilities that matter very
// differently: the rule is broken and letting things through, or the probe never
// made it into dist/. Checking the bytes rather than trusting the rule's silence.

import { readFileSync, readdirSync, statSync } from "node:fs";

const DIST = "D:/src/cc-switch-main/dist/assets";
const files = readdirSync(DIST).filter(
  (x) => /\.(js|css)$/.test(x) && statSync(`${DIST}/${x}`).size > 0,
);

for (const needle of ["SubRouter", "30% of direct API cost", "clientsTitle"]) {
  const hits = files.filter((f) => readFileSync(`${DIST}/${f}`, "utf8").includes(needle));
  console.log(`${needle}: ${hits.length} file(s)${hits.length ? " → " + hits.join(", ") : ""}`);
}
