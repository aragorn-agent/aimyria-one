// Drop the model-vendor enumeration from the pricing-sync description.
//
// The sentence read, in effect, "Selects {{count}} recent Claude, GPT, Gemini,
// Grok, DeepSeek, Qwen, MiMo, LongCat, Kimi, MiniMax, GLM…" — a list of every
// model family the pricing source tracks.
//
// It is a description of a real feature, not a competitor shelf: models.dev's
// sync picks recent models, the names are data, and the user's own model set is
// what gets priced. So this is not the bug the brand gate was catching.
//
// It is removed anyway, for a different reason: it tells the user nothing. The
// number matters — how many models get synced — and the names are an internal
// detail that also happens to be a list of other companies. Four locales, one
// sentence each, and none of them lose meaning.

import { readFileSync, writeFileSync } from "node:fs";

const LOCALES = {
  zh: "自动选择 {{count}} 个近期常用模型。可自行增删。",
  en: "Selects {{count}} recent common models. Add or remove any of them.",
  ja: "最近よく使われるモデルを {{count}} 件自動選択します。追加・削除は自由です。",
  "zh-TW": "自動選擇 {{count}} 個近期常用模型。可自行增删。",
};

let changed = 0;
for (const [loc, text] of Object.entries(LOCALES)) {
  const p = `D:/src/cc-switch-main/src/i18n/locales/${loc}.json`;
  const json = JSON.parse(readFileSync(p, "utf8"));
  const target = json?.usage?.modelsDevAutoSync;
  if (!target?.commonModelsDescription) {
    console.error(`${loc}: commonModelsDescription not found`);
    process.exit(1);
  }
  if (target.commonModelsDescription === text) continue;
  target.commonModelsDescription = text;
  writeFileSync(p, `${JSON.stringify(json, null, 2)}\n`);
  changed += 1;
}

console.log(`rewrote ${changed} locale(s)`);
if (changed === 0) console.log("(all already in the target wording)");
