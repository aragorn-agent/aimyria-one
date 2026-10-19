// Print the code around the URL-encoded brand-coloured SVG inside the bundle.
//
// The string is `…fill='%23D97757' fill-rule='nonzero'…%3c/svg%3e` — the upstream
// logo recoloured to our terracotta. Something is still pulling it in after the
// sidebar and about-page imports were removed, so this shows the surrounding
// minified code, which is enough to name the module it came from.

import { readFileSync, readdirSync } from "node:fs";

const DIST = "D:/src/cc-switch-main/dist/assets";
// The LARGEST index-*.js. `readdirSync` order put a 1 KB chunk first, so the first
// version of this script reported "0 occurrences" while the string sat in the 4.7 MB
// main bundle — the same class of error as the PowerShell `**` glob that reported
// "0 references" for an import that was still in the file.
const candidates = readdirSync(DIST)
  .filter((x) => /^index-.*\.js$/.test(x))
  .map((x) => ({ x, size: readFileSync(`${DIST}/${x}`).length }))
  .sort((a, b) => b.size - a.size);
const f = candidates[0].x;
console.log(`reading ${f} (${(candidates[0].size / 1024 / 1024).toFixed(2)} MB)`);
const t = readFileSync(`${DIST}/${f}`, "utf8");

const needle = "D97757";
let at = -1;
let n = 0;
while ((at = t.indexOf(needle, at + 1)) !== -1) {
  n += 1;
  const start = Math.max(0, at - 200);
  console.log("");
  console.log(`#${n} at offset ${at}`);
  console.log("  " + t.slice(start, at + 60).replace(/\s+/g, " "));
}
console.log("");
console.log(`total ${n} occurrence(s) in ${f}`);
