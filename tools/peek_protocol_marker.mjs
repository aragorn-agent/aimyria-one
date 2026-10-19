// Print the exact bytes around the protocol detection table, so the whitelist
// marker can be written against what is actually there rather than what I assume
// the minifier produced.

import { readFileSync, readdirSync, statSync } from "node:fs";

const DIST = "D:/src/cc-switch-main/dist/assets";
const f = readdirSync(DIST).find(
  (x) => x.endsWith(".js") && statSync(`${DIST}/${x}`).size > 1_000_000,
);
if (!f) {
  console.error("no bundle found");
  process.exit(1);
}
const t = readFileSync(`${DIST}/${f}`, "utf8");

for (const needle of ['"deepseek"', "deepseek.com", "bigmodel.cn"]) {
  const i = t.indexOf(needle);
  if (i === -1) {
    console.log(`${needle}: not found`);
    continue;
  }
  console.log(`${needle} @${i}:`);
  console.log("  " + JSON.stringify(t.slice(Math.max(0, i - 70), i + 80)));
}
