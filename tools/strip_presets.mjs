// Reduce every copied provider-preset list to the single relay this product ships.
//
// # What was there
//
// Upstream ships 16 preset files, 24,459 lines, 560 entries, 28 vendor names — the
// whole competitor relay catalogue, carrying referral-grade metadata (family,
// planKey, regionKey, primePartner, providerKey). All of it came across with the
// M22 frontend copy.
//
// The brand gate has a rule for exactly this — "No competitor presets are seeded" —
// and it passed, because it reads `src-tauri/src/database/dao/providers_seed.rs`
// and nothing else. We emptied that file in M19. A rule that checks one file and
// calls the whole question answered is the same failure the palette rule was.
//
// # Two versions of this script were wrong before this one
//
// 1. Matched every top-level `const X = [...]` in `src/config`. That also took
//    `OPENCODE_PRESET_MODEL_VARIANTS` and friends — model catalogues with no
//    presets in them — and typecheck went to 60 errors.
// 2. Matched by binding name, which was right, but rebuilt the file by string
//    offsets. The reconstruction swallowed the whole array *and* the `];`,
//    which is how `opencodeProviderPresets.ts` ended up 35 lines with its export
//    gone.
//
// This one works line by line: find the binding line, find the `];` that closes it
// at column zero, replace the lines between. Nothing else in the file moves.
//
// Run `node tools/strip_presets.mjs --check` to count without writing.

import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = "D:/src/cc-switch-main";
const CFG = join(ROOT, "src/config");
const checkOnly = process.argv.includes("--check");

const COMPETITORS = [
  "DeepSeek", "硅基流动", "SiliconFlow", "Novita", "Moonshot", "Kimi",
  "智谱", "Zhipu", "GLM", "豆包", "Doubao", "通义", "Qwen", "MiniMax",
  "PPIO", "LongCat", "Longcat", "MiMo", "Therouter", "Subrouter",
  "Moark", "Qianfan", "PPInfra", "TokenFlux", "Dmxapi", "302.AI",
  "AiHubMix", "JieKou", "OpenRouter", "Volcengine", "腾讯云", "百度云",
];

/**
 * What replaces the array.
 *
 * Option A — empty. An empty array satisfies any element type, so all ten files
 * keep compiling without a per-client preset shape: `GeminiProviderPreset` wanting
 * no `isOfficial` and `OpenCodeProviderConfig` wanting `npm/options/models` stop
 * mattering when there is nothing to type-check.
 *
 * That was the whole reason option B (one hand-written Aimyria entry per file)
 * failed: it needed seven different shapes, which is writing code rather than
 * removing it.
 */
const REPLACEMENT = "[]";

/** A binding whose identifier ends in `Presets` or `PresetDefinitions`. */
function isPresetBinding(name) {
  return /(Presets|PresetDefinitions)$/.test(name);
}

const files = readdirSync(CFG)
  .map((e) => join(CFG, e))
  .filter((p) => statSync(p).isFile() && /ProviderPresets\.ts$/.test(p));

let goneEntries = 0;
let goneLines = 0;
const goneVendors = new Set();
const touched = [];
const skipped = [];

for (const file of files) {
  const lines = readFileSync(file, "utf8").split("\n");

  // Binding line: `const name ... = [` or `export const name ... = [`, possibly with
  // a trailing comma in a multiline statement.
  const starts = [];
  for (let i = 0; i < lines.length; i++) {
    const m = lines[i].match(/^(?:export\s+)?const\s+(\w+)\s*(?::[^=]*)?=\s*\[/);
    if (!m) continue;
    if (!isPresetBinding(m[1])) {
      skipped.push(m[1]);
      continue;
    }
    // Find the `];` that closes it. Only a `];` at column zero closes a top-level
    // binding; entries inside the array are indented.
    let close = -1;
    for (let j = i + 1; j < lines.length; j++) {
      if (/^\];\s*,?\s*$/.test(lines[j])) {
        close = j;
        break;
      }
    }
    if (close === -1) {
      console.error(`  ${file}: no closing "];" found for ${m[1]} after line ${i + 1}`);
      process.exit(1);
    }
    starts.push({ name: m[1], open: i, close });
  }

  if (starts.length === 0) continue;

  for (const s of starts) {
    const body = lines.slice(s.open + 1, s.close).join("\n");
    goneEntries += (body.match(/\n\s*\{\s*\n|\n\s*\{[^\n]*name:/g) || []).length;
    for (const v of COMPETITORS) if (body.includes(v)) goneVendors.add(v);
  }
  goneLines += lines.length;
  touched.push({ file: file.replace(CFG + "\\", ""), bindings: starts.length });

  if (checkOnly) continue;

  // Rebuild from the last binding backwards so earlier line numbers stay valid.
  //
  // Replace the binding line *and* everything up to and including the closing
  // `];`. Splicing only between them leaves the original `]` in place, which
  // yields `[[]]` — an array holding an empty array, and exactly the
  // "Type '[]' is not assignable to type 'ClaudeDesktopProviderPreset'" error
  // that produced.
  let out = lines.slice();
  for (const s of [...starts].reverse()) {
    const decl = out[s.open].replace(/=\s*\[.*$/, "=").trimEnd();
    out.splice(s.open, s.close - s.open + 1, `${decl} ${REPLACEMENT};`);
  }
  writeFileSync(file, out.join("\n"));
}

console.log(checkOnly ? "would strip:" : "stripped:");
console.log(`  files    : ${touched.length}`);
console.log(`  bindings : ${touched.reduce((n, t) => n + t.bindings, 0)}`);
console.log(`  entries  : ${goneEntries}`);
console.log(`  lines    : ${goneLines.toLocaleString()}`);
console.log(`  vendors  : ${goneVendors.size}`);
console.log("");
for (const t of touched) console.log(`  ${String(t.bindings).padStart(2)} binding(s)  ${t.file}`);
if (skipped.length) {
  console.log("");
  console.log("left alone (not preset lists):");
  for (const s of [...new Set(skipped)]) console.log(`  ${s}`);
}
console.log("");
console.log("vendors removed:");
console.log("  " + [...goneVendors].sort().join(", "));
