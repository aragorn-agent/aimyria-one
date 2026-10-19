// Add the About-screen link labels to every locale.
//
// The About screen used to be upstream's: it linked to ccswitch.io and the
// upstream GitHub repository, and asked for a star there. The product now shows
// its own site, docs, support and source, plus the MIT attribution the licence
// requires. Those four labels are new strings, so they are added next to
// `aboutHint` rather than appended at the end of the file (JSON round-tripping
// the 200 KB locale files would rewrite every line).
//
// Idempotent: keys already present are left untouched.

import { readFileSync, writeFileSync } from "node:fs";

const LABELS = {
  zh: {
    docs: "使用文档",
    support: "获取支持",
    source: "源代码",
    license: "MIT 许可",
    attribution: "基于 CC Switch（MIT，Jason Young）修改",
  },
  "zh-TW": {
    docs: "使用文件",
    support: "取得支援",
    source: "原始碼",
    license: "MIT 授權",
    attribution: "基於 CC Switch（MIT，Jason Young）修改",
  },
  en: {
    docs: "Documentation",
    support: "Support",
    source: "Source code",
    license: "MIT license",
    attribution: "Based on CC Switch (MIT) by Jason Young",
  },
  ja: {
    docs: "ドキュメント",
    support: "サポート",
    source: "ソースコード",
    license: "MIT ライセンス",
    attribution: "CC Switch（MIT、Jason Young）を基に改修",
  },
};

for (const [lang, labels] of Object.entries(LABELS)) {
  const path = `src/i18n/locales/${lang}.json`;
  let text = readFileSync(path, "utf8");
  const match = text.match(/([ \t]*)"aboutHint":\s*"[^"]*",(\r?\n)/);
  if (!match) throw new Error(`${lang}: aboutHint anchor not found in ${path}`);
  const indent = match[1];
  const eol = match[2];

  const settings = JSON.parse(text).settings ?? {};
  // Check the *settings* object, not the whole file: `docs` and `source`
  // already exist in other sections (MCP docs, update source), and a global
  // text search therefore skipped adding them where this screen looks them up.
  const missing = Object.entries(labels).filter(([key]) => !(key in settings));
  if (missing.length === 0) {
    console.log(`${lang}: already present`);
    continue;
  }

  const block = missing
    .map(([key, value]) => `${indent}${JSON.stringify(key)}: ${JSON.stringify(value)},${eol}`)
    .join("");
  text = text.replace(match[0], match[0] + block);
  writeFileSync(path, text);
  console.log(`${lang}: added ${missing.map(([k]) => k).join(", ")}`);
}
