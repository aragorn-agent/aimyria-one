// Replace the competitor names in the preset search placeholder.
//
// `搜索名称或网址，例如 kimi、智谱、openrouter` — the search box told the user what
// to type by naming three relay vendors. With the presets gone there is nothing to
// search by vendor any more, and the examples pointed at companies whose products
// this tool does not configure.
//
// The placeholder still has a job: it says the box matches both a name and a URL.
// It just needs an example that exists here.

import { readFileSync, writeFileSync } from "node:fs";

const TEXT = {
  zh: "搜索名称或网址",
  en: "Search name or URL",
  ja: "名前または URL で検索",
  "zh-TW": "搜尋名稱或網址",
};

let changed = 0;
for (const [loc, text] of Object.entries(TEXT)) {
  const p = `D:/src/cc-switch-main/src/i18n/locales/${loc}.json`;
  const json = JSON.parse(readFileSync(p, "utf8"));
  const target = json?.providerPreset;
  if (!target?.searchPlaceholder) {
    console.error(`${loc}: providerPreset.searchPlaceholder not found`);
    process.exit(1);
  }
  if (target.searchPlaceholder === text) continue;
  target.searchPlaceholder = text;
  writeFileSync(p, `${JSON.stringify(json, null, 2)}\n`);
  changed += 1;
}

console.log(`rewrote ${changed} locale(s)`);
if (changed === 0) console.log("(all already in the target wording)");
