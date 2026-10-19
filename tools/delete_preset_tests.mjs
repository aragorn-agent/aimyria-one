// Delete the tests for the presets we removed.
//
// # Why delete rather than fix
//
// Option 1A emptied the preset arrays: 24,459 → 1,433 lines, 560 entries → 0. The
// tests that covered them assert those entries exist, so every one of them fails
// with `expected undefined to be defined` or a TypeError on the missing entry.
//
// Fixing them would mean writing a preset list back, which is the thing the user
// removed. Leaving them red is worse: a suite that is always red teaches you to
// ignore red, and the real regressions hide inside the noise.
//
// # The guard
//
// A file is deleted only if EVERY failure in it matches the "preset is absent"
// signature — an undefined lookup, a TypeError reading a field off undefined, or a
// length/equality check against a missing preset. One genuine failure — an import
// error, a TypeError from a typo, a mismatch in something that exists — and the
// file is kept and reported. This project's brand gate was vacuously passing twice
// because a rule checked one file and I read its verdict as a verdict on all of
// them; this guard is the same idea applied before deleting rather than after.
//
// The list is written to tools/deleted-preset-tests.md so the deletion is
// reviewable and reversible.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync, unlinkSync } from "node:fs";
import { join } from "node:path";

const ROOT = "D:/src/cc-switch-main";

// Only these kinds of file are candidates. `ProviderForm.presetRows.golden` and
// `ProviderPresetSelector` are included because their subject is the preset list
// itself, not the form.
// Files whose subject is the preset list. The three explicit names are preset
  // tests that do not carry the word: `piThinkingProfiles` iterates
  // `piProviderPresets`, the golden file snapshots the preset rows, and
  // `ProviderPresetSelector` lays out preset families.
  const EXTRA_NAMES = new Set([
    "piThinkingProfiles.test.ts",
    "ProviderForm.presetRows.golden.test.tsx",
    "ProviderPresetSelector.test.tsx",
  ]);
  const CANDIDATE = /(?:^|\/)(?:[a-zA-Z]*[Pp]resets?[A-Za-z.]*\.test\.(?:ts|tsx))$/;
  const isPresetFile = (r) =>
    CANDIDATE.test(`/${r}`) || EXTRA_NAMES.has(r.split("/").pop());

// Failure shapes that mean "the preset this test wanted is not there".
const ABSENT = [
  /expected undefined to be (?:defined|'|"|be)/,
  /Cannot read properties of undefined/,
  /expected undefined to match object/,
  /expected undefined to deeply equal/,
  /expected \d+ to be 0/,
  /expected 0 to be/,
  /toHaveLength\(0\)/,
  /expected \[\] to/,
  /preset .*expected undefined/i,
  /undefined to match/i,
  /expected .* to have a length of 0/i,
  /to be defined/,
  // A `describe.each(presets)` over an empty array registers no tests at all, and
  // vitest reports the empty suite as an error rather than as a failed assertion.
  /No test found in suite/,
  // `预设按钮「X」应唯一: expected [] to have a length of 1 but got +0`
  /expected \[\] to have a length/,
  /to have a length of \d+ but got/,
  /expected \[\] to deeply equal/,
  // `expect(presets.some(x => ...)).toBe(true)` over an empty array.
  /expected false to be true/,
  // An empty object where the preset's fields were expected.
  /expected \{\} to deeply equal/,
  /expected \{\} to match object/,
  // Vitest's own message when a value is undefined and an assertion needs a value.
  /undefined and \w+ is invalid for this assertion/,
];

// Weak signals. `expected 0 to be greater than 0` is what an empty preset array
// produces — and it is also what a genuine "this collection should not be empty"
// bug produces. So it only counts for a file that also shows a strong signal;
// deleting a file on it alone is the kind of quiet mistake this script exists to
// stop.
const WEAK = [/expected 0 to be greater than 0/, /ArrayContaining/];

function isAbsent(line) {
  return ABSENT.some((re) => re.test(line));
}

function isWeak(line) {
  return WEAK.some((re) => re.test(line));
}

console.log("running preset tests to classify them...\n");

let out;
try {
  out = execFileSync(
    "npx",
    ["vitest", "run", "tests/config", "src/config", "tests/components/ProviderForm.presetRows.golden.test.tsx", "tests/components/ProviderPresetSelector.test.tsx"],
    { cwd: ROOT, encoding: "utf8", maxBuffer: 64 * 1024 * 1024, shell: true },
  );
} catch (e) {
  // vitest exits non-zero when tests fail, which is the expected case here.
  out = `${e.stdout || ""}${e.stderr || ""}`;
}

const clean = out.replace(/\x1b\[[0-9;]*m/g, "");
const lines = clean.split(/\r?\n/);

// Group every error line under the file it belongs to. vitest prints the file on
// the FAIL line, then its errors indented beneath.
const perFile = new Map();
let current = null;
for (const line of lines) {
  const m = line.match(/^\s*FAIL\s+(\S+\.test\.(?:ts|tsx))/);
  if (m) {
    current = m[1];
    if (!perFile.has(current)) perFile.set(current, []);
    continue;
  }
  if (!current) continue;
  if (/AssertionError|TypeError|ReferenceError|Error:/.test(line)) {
    perFile.get(current).push(line.trim());
  }
}

const deleted = [];
const kept = [];

for (const [file, errors] of perFile) {
  const rel = file.replace(/\\/g, "/").replace(/^.*\/cc-switch-main\//, "");
  // vitest reports some files with a path that does not resolve from the repo
  // root — an absolute path from a different drive, or a name with a column
  // suffix. Fall back to finding the file by basename before giving up, or these
  // would sit in "kept" forever for a reason that has nothing to do with the
  // preset question.
  let abs = join(ROOT, rel);
  if (!existsSync(abs)) {
    const base = rel.split("/").pop();
    const dirs = [join(ROOT, "tests/config"), join(ROOT, "src/config"), join(ROOT, "tests/components"), join(ROOT, "tests/lib")];
    for (const d of dirs) {
      const cand = join(d, base);
      if (existsSync(cand)) {
        abs = cand;
        break;
      }
    }
  }
  const realRel = abs.replace(/\\/g, "/").replace(/^.*\/cc-switch-main\//, "");
  if (!isPresetFile(realRel) && !isPresetFile(rel)) {
    kept.push({ rel: realRel, why: "not a preset file", errors: errors.length });
    continue;
  }
  if (errors.length === 0) {
    kept.push({ rel, why: "no error lines parsed — will not delete blind", errors: 0 });
    continue;
  }
  const notAbsent = errors.filter((e) => !isAbsent(e));
  // Every remaining failure must be weak, and the file must carry at least one
  // strong signal — otherwise the weak ones could be a real bug.
  const hasStrong = errors.some(isAbsent);
  const notWeakEither = notAbsent.filter((e) => !isWeak(e));
  if (notWeakEither.length > 0) {
    kept.push({ rel, why: `${notWeakEither.length}/${errors.length} failures match no known shape`, errors: errors.length, sample: notWeakEither.slice(0, 2) });
    continue;
  }
  if (notAbsent.length > 0 && !hasStrong) {
    kept.push({ rel, why: `only weak signals (${notAbsent.length}) — could be a real empty-collection bug`, errors: errors.length, sample: notAbsent.slice(0, 2) });
    continue;
  }
  if (!existsSync(abs)) {
    kept.push({ rel: realRel, why: "path not found", errors: errors.length });
    continue;
  }
  unlinkSync(abs);
  deleted.push({ rel: realRel, errors: errors.length });
}

const lines2 = [
  "# 删掉的预设测试（1A 决定的必然结果）",
  "",
  `删除时间：2026-10-14　共 **${deleted.length}** 个文件。`,
  "",
  "预设数组按业主 1A 决定清空（24,459 → 1,433 行，560 → 0 条）。这些测试断言的正是",
  "那些条目，所以全部以「找不到预设」的形式失败。删它们不是掩盖问题——被测的功能",
  "本身已经不存在了。留着它们会让测试套件永久红，真回归淹没在噪音里。",
  "",
  "**守卫**：每个文件只有在它的**全部**失败都是「预设不存在」形态时才删。",
  "出现任何别的失败（导入错误、字段拼写、对存在值的断言不符），文件保留。",
  "",
  "| 文件 | 失败断言数 |",
  "|---|---|",
  ...deleted.map((d) => `| \`${d.rel}\` | ${d.errors} |`),
  "",
];

if (kept.length) {
  lines2.push("## 保留（守卫拦下的）", "", "| 文件 | 保留原因 |", "|---|---|");
  for (const k of kept) lines2.push(`| \`${k.rel}\` | ${k.why} |`);
  lines2.push("");
}

writeFileSync(join(ROOT, "tools/deleted-preset-tests.md"), lines2.join("\n"));

console.log(`deleted ${deleted.length} preset test files`);
for (const d of deleted) console.log(`  - ${d.rel}  (${d.errors} assertions)`);
if (kept.length) {
  console.log(`\nkept ${kept.length}:`);
  for (const k of kept) console.log(`  - ${k.rel}  ${k.why}`);
}
console.log("\ntools/deleted-preset-tests.md");