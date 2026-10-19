// Audit what provider presets shipped with the copied frontend.
//
// The brand gate's "no competitor presets are seeded" rule reads
// `src-tauri/src/database/dao/providers_seed.rs`, which we emptied. It does not
// look at `src/config/` — so upstream's 23,830 lines of competitor presets came
// across with the M22 copy and the rule stayed green.
//
// A rule that checks one file and calls the whole question answered is the same
// failure as the palette rule was: green, and wrong.
//
// Also counts the CLI clients in the sidebar, because "which clients does this
// build actually configure" is a separate question from "which relays does it
// offer".

import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = "D:/src/cc-switch-main";
const CFG = join(ROOT, "src/config");

function walk(dir, filter) {
  const out = [];
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    if (statSync(p).isDirectory()) out.push(...walk(p, filter));
    else if (filter(p)) out.push(p);
  }
  return out;
}

// Relay vendors other than Aimyria. Names as they appear in upstream's preset
// files — display names and slugs both, because a preset can carry one and be
// keyed on the other.
const COMPETITORS = [
  "DeepSeek", "硅基流动", "SiliconFlow", "Novita", "Moonshot", "Kimi",
  "智谱", "Zhipu", "GLM", "豆包", "Doubao", "通义", "Qwen", "MiniMax",
  "PPIO", "LongCat", "Longcat", "MiMo", "mimo", "Therouter", "Subrouter",
  "Moark", "Qianfan", "PPInfra", "TokenFlux", "Dmxapi", "DmxAPI",
  "302.AI", "AiHubMix", "Aihubmix", "Jiekou", "接口", "OpenRouter",
  "Openrouter", "火山", "Volcengine", "腾讯云", "百度云", "Baidu",
];

const presetFiles = walk(CFG, (p) => /[Pp]reset.*\.ts$/.test(p) && !/\.test\./.test(p));

const rows = [];
let totalLines = 0;
let totalPresetEntries = 0;
const allNames = new Map();

for (const file of presetFiles) {
  const text = readFileSync(file, "utf8");
  const lines = text.split("\n").length;
  totalLines += lines;

  // Every preset entry is an object literal with an `id`. Counting `id:` lines
  // at any nesting depth over-counts slightly (settings objects also have ids),
  // but it is the right order of magnitude and it is consistent across files.
  const entries = (text.match(/^\s*id:\s*["'`]/gm) || []).length;
  totalPresetEntries += entries;

  const names = COMPETITORS.filter((n) => text.includes(n));
  if (names.length) {
    for (const n of names) allNames.set(n, (allNames.get(n) || 0) + 1);
  }
  rows.push({
    file: file.replace(CFG + "\\", "").replaceAll("\\", "/"),
    lines,
    entries,
    vendors: names.length,
  });
}

rows.sort((a, b) => b.lines - a.lines);

const out = [
  "# 复制进来的 provider 预设",
  "",
  `- 预设文件：**${presetFiles.length}** 个`,
  `- 总行数：**${totalLines.toLocaleString()}**`,
  `- \`id:\` 条目（粗略）：**${totalPresetEntries}**`,
  `- 出现的第三方中转名：**${allNames.size}** 种`,
  "",
  "## 按文件",
  "",
  "| 文件 | 行数 | 条目 | 含第三方 |",
  "| --- | ---: | ---: | ---: |",
  ...rows.map((r) => `| \`${r.file}\` | ${r.lines} | ${r.entries} | ${r.vendors} |`),
  "",
  "## 第三方名出现次数",
  "",
  ...([...allNames.entries()].sort((a, b) => b[1] - a[1]).map(([n, c]) => `- ${n} ×${c}`)),
  "",
  "## 与此相反的：Rust 侧",
  "",
  "`providers_seed.rs` 的 `OFFICIAL_SEEDS` 已清空——品牌门禁查的就是它，所以",
  "「No competitor presets are seeded」一直是 PASS。它没看 `src/config/`。",
  "",
].join("\n");

writeFileSync(join(ROOT, "tools/preset-audit.md"), out);

console.log(`preset files : ${presetFiles.length}`);
console.log(`total lines  : ${totalLines.toLocaleString()}`);
console.log(`id entries   : ${totalPresetEntries}`);
console.log(`vendors      : ${allNames.size}`);
console.log("");
console.log("worse offenders:");
for (const r of rows.slice(0, 8)) console.log(`  ${String(r.lines).padStart(6)}  ${r.file}  (${r.vendors} vendors)`);
