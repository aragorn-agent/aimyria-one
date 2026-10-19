import { readFileSync, writeFileSync } from "node:fs";
const files = ["zh", "zh-TW", "en", "ja"].map((l) => `src/i18n/locales/${l}.json`);
for (const f of files) {
  let s = readFileSync(f, "utf8");
  const before = (s.match(/Aimyria/g) || []).length;
  s = s.replaceAll("Aimyria 一键配置工具", "AimyriA go");
  s = s.replaceAll("Aimyria One", "AimyriA go");
  s = s.replaceAll("Aimyria", "AimyriA go");
  // The relay is still Aimyria; only the desktop product is AimyriA go.
  s = s.replaceAll("AimyriA go 中转站", "Aimyria 中转站");
  s = s.replaceAll("AimyriA go 中转", "Aimyria 中转");
  s = s.replaceAll("AimyriA go 账号", "Aimyria 账号");
  writeFileSync(f, s);
  const after = (s.match(/AimyriA go/g) || []).length;
  console.log(f, "Aimyria", before, "->", "AimyriA go", after);
}
