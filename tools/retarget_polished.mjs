import { readFileSync, writeFileSync } from "node:fs";
const files = ["src/index.css", "src/styles.css", "src/components/aimyria/Quickstart.tsx", "src/components/DeepLinkImportDialog.tsx", "src/components/settings/AboutSection.tsx"];
const map = [
  [/#b9d5ec/gi, "#e9f1f9"],
  [/#a6c8e4/gi, "#f5f9fd"],
  [/#d3e6f6/gi, "#ffffff"],
  [/#dceaf7/gi, "#dce9f5"],
  [/#3f6488/gi, "#8fa9c4"],
  [/#eaf3fb/gi, "#f2f7fc"],
  [/rgba\(185,\s*213,\s*236/gi, "rgba(233, 241, 249"],
];
for (const f of files) {
  let s = readFileSync(f, "utf8");
  for (const [re, to] of map) s = s.replace(re, to);
  writeFileSync(f, s);
  console.log(f);
}
