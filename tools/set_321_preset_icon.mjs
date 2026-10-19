import { readFileSync, writeFileSync } from "node:fs";
const files = [
  "src/config/claudeProviderPresets.ts",
  "src/config/codexProviderPresets.ts",
  "src/config/geminiProviderPresets.ts",
  "src/config/claudeDesktopProviderPresets.ts",
  "src/config/universalProviderPresets.ts",
];
for (const f of files) {
  let s = readFileSync(f, "utf8");
  const n = (s.match(/icon: "generic"/g) || []).length;
  s = s.replaceAll('icon: "generic"', 'icon: "321ai"');
  writeFileSync(f, s);
  console.log(f, n);
}
