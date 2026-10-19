// List every occurrence of a brand in the main bundle, with the 30 characters
// before it, so the brand rule's whitelist shapes can be checked against real text
// rather than assumed.
//
// The rule went red on "MiniMax" while the context it printed clearly showed
// `label:"MiniMax Code"` — a shape it claims to allow. So either the shape test is
// wrong, or the brand has a second occurrence that is not that shape and the
// `every()` correctly refuses to whitelist the file. This prints all of them so the
// second occurrence, if it exists, is visible.

import { readFileSync, readdirSync, statSync } from "node:fs";

const DIST = "D:/src/cc-switch-main/dist/assets";
const f = readdirSync(DIST)
  .filter((x) => /^index-.*\.js$/.test(x))
  .map((x) => ({ x, size: statSync(`${DIST}/${x}`).size }))
  .sort((a, b) => b.size - a.size)[0].x;
const t = readFileSync(`${DIST}/${f}`, "utf8");

const brand = process.argv[2] || "MiniMax";
console.log(`${brand} in ${f} (${(statSync(`${DIST}/${f}`).size / 1048576).toFixed(2)} MB)`);

let i = -1;
let n = 0;
while ((i = t.indexOf(brand, i + 1)) !== -1) {
  n += 1;
  const before = t.slice(Math.max(0, i - 34), i);
  const isLabel = new RegExp(`label:\\s*["']${brand}`).test(
    t.slice(Math.max(0, i - 30), i + brand.length + 30),
  );
  const isCatalogue = /"(?:id|name)":"$/.test(before);
  const tag = isLabel ? "LABEL" : isCatalogue ? "CATALOGUE" : "*** UNSHAPED ***";
  console.log(`  #${n} ${tag}`);
  console.log(`     before: ${JSON.stringify(before)}`);
  if (n >= 10) {
    console.log("  …(stopped at 10)");
    break;
  }
}
console.log(`  total ${n}`);
