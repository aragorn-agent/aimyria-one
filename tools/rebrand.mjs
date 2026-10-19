// Rebrand the copied frontend from "CC Switch" to Aimyria.
//
// 384 occurrences (369 in the four locale files, 15 in TypeScript), and three of
// them are not really the product name. A blind find-and-replace would break the
// build in ways that typecheck cannot see:
//
//   1. `errorUtils.ts` matches `message.includes("changed outside CC Switch")`.
//      That string is produced by the *Rust* side. Retarget the TypeScript
//      without retargeting Rust and every "file changed outside the app" error
//      stops being recognised — the message still renders, so nothing looks
//      broken, but the app no longer explains that it lost a race.
//   2. `errorUtils.ts` also matches the on-disk config directory, `~/.cc-switch`.
//      Renaming that string would point the app at a directory it never writes,
//      so every setting would silently reset on upgrade.
//   3. The `title` value in the locale files is also what the taskbar and window
//      chrome show, and `tauri.conf.json`'s window title is Aimyria's already —
//      the two would disagree.
//
// So: the product *name* is replaced, the config *directory* and the
// cross-language error *matcher* are not. Run `node tools/rebrand.mjs --check`
// to see what would change without writing.

import { readFileSync, writeFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const ROOT = "D:/src/cc-switch-main";
const FROM = "CC Switch";
const TO = "Aimyria";

const checkOnly = process.argv.includes("--check");

function walk(dir, filter) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) out.push(...walk(p, filter));
    else if (filter(p)) out.push(p);
  }
  return out;
}

// Occurrences that must keep the old name, and why.
const KEEP = [
  {
    file: "src/utils/errorUtils.ts",
    reason:
      "line 48 is `message.includes(\"changed outside CC Switch\")`, matching text the Rust side emits (pi_config, pi_prompt_files, prompt). Renaming one side only silently disables the detection",
  },
  {
    file: "src/i18n/locales",
    reason: "only the `headerValuePlaceholder` key names the config directory (~/.cc-switch)",
  },
];

const edits = [];
let total = 0;

// ── Locale files ─────────────────────────────────────────────────────────────
// Every `CC Switch` here is the product name in prose, except one key that
// stands for the config directory path.
const locales = walk(join(ROOT, "src/i18n"), (p) => p.endsWith(".json"));
for (const file of locales) {
  const text = readFileSync(file, "utf8");
  const lines = text.split("\n");
  let n = 0;
  for (let i = 0; i < lines.length; i++) {
    if (!lines[i].includes(FROM)) continue;
    // The placeholder shows a filesystem path, not the product.
    if (lines[i].includes("headerValuePlaceholder")) continue;
    if (checkOnly) {
      edits.push({ file, line: i + 1, from: lines[i].trim().slice(0, 90) });
      n += 1;
      continue;
    }
    lines[i] = lines[i].split(FROM).join(TO);
    n += 1;
  }
  if (n && !checkOnly) writeFileSync(file, lines.join("\n"));
  total += n;
}

// ── TypeScript sources ───────────────────────────────────────────────────────
const sources = walk(join(ROOT, "src"), (p) => /\.tsx?$/.test(p));
for (const file of sources) {
  const rel = file.replace(ROOT + "\\", "").replaceAll("\\", "/");
  if (KEEP.some((k) => k.file === rel)) continue;
  const text = readFileSync(file, "utf8");
  if (!text.includes(FROM)) continue;
  if (checkOnly) {
    for (const m of text.matchAll(new RegExp(`.{0,60}${FROM}.{0,40}`, "g"))) {
      edits.push({ file: rel, line: 0, from: m[0].replace(/\n/g, " ") });
    }
    total += (text.match(new RegExp(FROM, "g")) || []).length;
    continue;
  }
  const next = text.split(FROM).join(TO);
  writeFileSync(file, next);
  total += (text.match(new RegExp(FROM, "g")) || []).length;
}

console.log(checkOnly ? "would replace:" : "replaced:");
console.log(`  ${total} occurrences`);
if (checkOnly) {
  for (const e of edits.slice(0, 20)) console.log(`  ${e.file}${e.line ? `:${e.line}` : ""}: ${e.from}`);
  if (edits.length > 20) console.log(`  … and ${edits.length - 20} more`);
}
console.log("");
console.log("deliberately left alone:");
for (const k of KEEP) console.log(`  ${k.file}\n      ${k.reason}`);
