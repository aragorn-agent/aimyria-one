// Which dependency supplies the `data:image/svg+xml,…` URIs that one component
// is rendering as visible text?
//
// The bundle has four of them; `src/` has none, so they come from a package. The
// first search missed them because pnpm nests real packages under
// `node_modules/.pnpm/<name>@<version>/node_modules/<name>/`.
//
// Reported as a list of candidate files with the matching line, so the next step is
// reading a real path rather than guessing.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const ROOT = "D:/src/cc-switch-main";
const NEEDLE = "height='1em'%20style='flex:none";
const LIMIT = 6;

function* walk(dir, depth = 0) {
  if (depth > 7) return;
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of entries) {
    const p = join(dir, e.name);
    if (e.isDirectory()) yield* walk(p, depth + 1);
    else if (/\.(m?js|cjs|ts)$/.test(e.name)) yield p;
  }
}

const found = [];
for (const p of walk(join(ROOT, "node_modules"))) {
  let text;
  try {
    text = readFileSync(p, "utf8");
  } catch {
    continue;
  }
  if (!text.includes(NEEDLE)) continue;
  const at = text.indexOf(NEEDLE);
  found.push({ p: p.replace(ROOT + "/", ""), at, around: text.slice(Math.max(0, at - 120), at + 60) });
  if (found.length >= LIMIT) break;
}

if (found.length === 0) {
  console.log("no match in node_modules");
} else {
  for (const f of found) {
    console.log(`\n${f.p}`);
    console.log(`  …${f.around.replace(/\s+/g, " ")}…`);
  }
}
