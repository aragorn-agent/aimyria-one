import { readFileSync, writeFileSync } from "node:fs";
for (const lang of ["zh", "zh-TW", "en", "ja"]) {
  const p = `src/i18n/locales/${lang}.json`;
  let s = readFileSync(p, "utf8");
  const before = s;
  s = s.replaceAll('"balance": "${{amount}}"', '"balance": "{{amount}}"');
  if (s !== before) { writeFileSync(p, s); console.log(lang, "fixed"); }
  else console.log(lang, "pattern not found");
}
