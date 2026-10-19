import { readFileSync, writeFileSync } from "node:fs";
const map = {
  "#d8b56a": "#C27849",
  "#e7c985": "#C9814F",
  "#e8c777": "#E0A176",
  "#c9a95f": "#C9814F",
  "#a67b2f": "#A85F36",
  "#8c6227": "#8A4F2C",
  "#f7d794": "#E0A176",
  "#f3efe6": "#FFFFFF",
  "#b9b2a3": "#8A8A88",
  "#999183": "#6B6B69",
  "#171614": "#0D0D0F",
  "#5c471c": "#3A2417",
  "#f5d993": "#E8C3A8",
  "#eee8da": "#E4E4E8",
  "#8f887b": "#6B6B69",
};
for (const f of ["src/components/aimyria/Quickstart.tsx", "src/components/DeepLinkImportDialog.tsx"]) {
  let s = readFileSync(f, "utf8");
  let n = 0;
  for (const [from, to] of Object.entries(map)) {
    const re = new RegExp(from, "gi");
    n += (s.match(re) || []).length;
    s = s.replace(re, to);
  }
  writeFileSync(f, s);
  console.log(f, "replaced", n);
}
