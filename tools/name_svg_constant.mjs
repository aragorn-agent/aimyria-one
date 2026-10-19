// Name the minified constant that holds the brand-coloured SVG, and show what
// module it sits in, so the importer can be found in `src/` rather than guessed at.

import { readFileSync, readdirSync } from "node:fs";

const DIST = "D:/src/cc-switch-main/dist/assets";
const f = readdirSync(DIST)
  .filter((x) => /^index-.*\.js$/.test(x))
  .map((x) => ({ x, size: readFileSync(`${DIST}/${x}`).length }))
  .sort((a, b) => b.size - a.size)[0].x;
const t = readFileSync(`${DIST}/${f}`, "utf8");

const at = t.indexOf("D97757");
const seg = t.slice(Math.max(0, at - 2000), at);

// The constant is `NAME="data:image/svg+xml,…`. Walk back to its opening quote.
const openQuote = seg.lastIndexOf('="data:image/svg+xml');
const beforeAssign = seg.slice(0, openQuote);
const nameMatch = beforeAssign.match(/([A-Za-z_$][\w$]*)\s*$/);

console.log(`bundle   : ${f}`);
console.log(`offset   : ${at}`);
console.log(`constant : ${nameMatch ? nameMatch[1] : "(not found)"}`);

// A module boundary in a Vite bundle is a comment or an import statement. The
// nearest one before the constant is the module it belongs to.
const chunk = seg.slice(-1600);
const boundaries = [
  ...chunk.matchAll(/\/\*[\s\S]{0,120}?\*\//g),
  ...chunk.matchAll(/from"[^"]+"/g),
];
const last = boundaries[boundaries.length - 1];
console.log(`module   : ${last ? last[0].slice(0, 100) : "(no boundary found nearby)"}`);

console.log("");
console.log("context before the constant:");
console.log("  " + beforeAssign.slice(-260).replace(/\s+/g, " "));
