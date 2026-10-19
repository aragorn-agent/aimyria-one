import { readFileSync, writeFileSync } from "node:fs";
const labels = {
  zh: ["MCP", "技能"],
  "zh-TW": ["MCP", "技能"],
  en: ["MCP", "Skills"],
  ja: ["MCP", "スキル"],
};
for (const [lang, [mcp, skills]] of Object.entries(labels)) {
  const p = `src/i18n/locales/${lang}.json`;
  let s = readFileSync(p, "utf8");
  const nav = JSON.parse(s).nav ?? {};
  if (nav.mcp && nav.skills) {
    console.log(lang, "already present");
    continue;
  }
  const m = s.match(/(\n(\s*)"quickstart":\s*"[^"]*",\n)/);
  if (!m) throw new Error(`${lang}: quickstart anchor missing`);
  const indent = m[2];
  const block = `${indent}"mcp": ${JSON.stringify(mcp)},\n${indent}"skills": ${JSON.stringify(skills)},\n`;
  s = s.replace(m[1], m[1] + block);
  writeFileSync(p, s);
  console.log(lang, "added");
}
