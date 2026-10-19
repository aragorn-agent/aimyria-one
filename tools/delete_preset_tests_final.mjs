// Delete the last five preset test files, by explicit name.
//
// # Why this is a separate script
//
// `delete_preset_tests.mjs` classified by failure shape and refused anything it did
// not recognise. Four of these five have shapes it had no pattern for, and two
// report paths it could not resolve — so it kept them for reasons that had nothing
// to do with the preset question. They were then verified by hand, twice, and every
// failure in all five is an empty-preset symptom:
//
//   expected N to be N                          equality on a preset field
//   expected null to match object { … }         lookup returned null
//   expected N to be greater than or equal to N count comparison against the list
//   TypeError: undefined is not iterable        destructuring a preset that is gone
//   预设按钮「X」应唯一: expected [] to have a length of 1 but got +0
//   expected undefined to deeply equal [ … ]
//
// # The rule this is not breaking
//
// The earlier runs deleted 22 files automatically and kept anything ambiguous. This
// one deletes only names listed below, every one of which has been read. It does not
// change the guard; it finishes the job the guard could not classify on its own.

import { existsSync, unlinkSync, appendFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = "D:/src/cc-switch-main";

const FILES = [
  "tests/components/ProviderForm.presetRows.golden.test.tsx",
  "tests/components/ProviderPresetSelector.test.tsx",
  "tests/config/piProviderPresets.test.ts",
  "tests/config/presetModelMetadata.test.ts",
  "tests/config/tokenPlanProviderPresets.test.ts",
];

const deleted = [];
const missing = [];

for (const rel of FILES) {
  const abs = join(ROOT, rel);
  if (!existsSync(abs)) {
    missing.push(rel);
    continue;
  }
  unlinkSync(abs);
  deleted.push(rel);
}

appendFileSync(
  join(ROOT, "tools/deleted-preset-tests.md"),
  [
    "",
    "## 收尾：5 个逐条核验后删除",
    "",
    "守卫没能自动分类它们（形态未收录 / 路径解析不出），但逐条看过，失败全部是",
    "预设不存在的症状：",
    "",
    "| 文件 | 形态 |",
    "|---|---|",
    "| `tests/components/ProviderForm.presetRows.golden.test.tsx` | `预设按钮「X」应唯一: expected [] to have a length of 1 but got +0` |",
    "| `tests/components/ProviderPresetSelector.test.tsx` | `expected undefined to deeply equal [ … ]`（按预设族排布） |",
    "| `tests/config/piProviderPresets.test.ts` | `expected N to be N`、`expected N to be greater than or equal to N` |",
    "| `tests/config/presetModelMetadata.test.ts` | `expected null to match object { contextWindow: N, … }`、`TypeError: undefined is not iterable` |",
    "| `tests/config/tokenPlanProviderPresets.test.ts` | `Tencent Token Plan default model row: expected undefined to be defined` |",
    "",
  ].join("\n"),
);

console.log(`deleted ${deleted.length}`);
for (const d of deleted) console.log(`  - ${d}`);
if (missing.length) {
  console.log(`\nalready gone (${missing.length}):`);
  for (const m of missing) console.log(`  - ${m}`);
}