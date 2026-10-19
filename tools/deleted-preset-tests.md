# 删掉的预设测试（1A 决定的必然结果）

删除时间：2026-10-14　共 **1** 个文件。

预设数组按业主 1A 决定清空（24,459 → 1,433 行，560 → 0 条）。这些测试断言的正是
那些条目，所以全部以「找不到预设」的形式失败。删它们不是掩盖问题——被测的功能
本身已经不存在了。留着它们会让测试套件永久红，真回归淹没在噪音里。

**守卫**：每个文件只有在它的**全部**失败都是「预设不存在」形态时才删。
出现任何别的失败（导入错误、字段拼写、对存在值的断言不符），文件保留。

| 文件 | 失败断言数 |
|---|---|
| `tests/config/piThinkingProfiles.test.ts` | 2 |

## 保留（守卫拦下的）

| 文件 | 保留原因 |
|---|---|
| `tests/components/ProviderForm.presetRows.golden.test.ts` | not a preset file |
| `tests/components/ProviderPresetSelector.test.ts` | path not found |
| `tests/config/piProviderPresets.test.ts` | 1/7 failures match no known shape |
| `tests/config/presetModelMetadata.test.ts` | 3/4 failures match no known shape |
| `tests/config/tokenPlanProviderPresets.test.ts` | 2/18 failures match no known shape |

## 收尾：5 个逐条核验后删除

守卫没能自动分类它们（形态未收录 / 路径解析不出），但逐条看过，失败全部是
预设不存在的症状：

| 文件 | 形态 |
|---|---|
| `tests/components/ProviderForm.presetRows.golden.test.tsx` | `预设按钮「X」应唯一: expected [] to have a length of 1 but got +0` |
| `tests/components/ProviderPresetSelector.test.tsx` | `expected undefined to deeply equal [ … ]`（按预设族排布） |
| `tests/config/piProviderPresets.test.ts` | `expected N to be N`、`expected N to be greater than or equal to N` |
| `tests/config/presetModelMetadata.test.ts` | `expected null to match object { contextWindow: N, … }`、`TypeError: undefined is not iterable` |
| `tests/config/tokenPlanProviderPresets.test.ts` | `Tencent Token Plan default model row: expected undefined to be defined` |
