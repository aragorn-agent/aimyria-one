import { readFileSync, writeFileSync } from "node:fs";
const files = ["src/index.css", "src/styles.css", "src/components/aimyria/Quickstart.tsx", "src/components/DeepLinkImportDialog.tsx", "src/components/settings/AboutSection.tsx"];
const map = [
  [/#c27849/gi, "#b9d5ec"],
  [/#bb6f41/gi, "#a6c8e4"],
  [/#c9814f/gi, "#d3e6f6"],
  [/#e0a176/gi, "#dceaf7"],
  [/#a85f36/gi, "#3f6488"],
  [/#f7ece4/gi, "#eaf3fb"],
  [/rgba\(194,\s*120,\s*73/gi, "rgba(185, 213, 236"],
  [/#7fa6c9/gi, "#7fa6c9"],
];
for (const f of files) {
  let s = readFileSync(f, "utf8");
  for (const [re, to] of map) s = s.replace(re, to);
  writeFileSync(f, s);
  console.log(f, "diamond retune");
}
