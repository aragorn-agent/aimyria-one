import { readFileSync, writeFileSync } from "node:fs";
const map = { "#d97757": "#c9814f", "#9a4f29": "#a85f36", "#ab5a30": "#c27849", "#f2f1ec": "#f5f5f7", "#141413": "#000000", "#9a9a92": "#8a8a88", "#2b2b2b": "#0d0d0f", "#fbfaf7": "#ffffff" };
let s = readFileSync("src/styles.css", "utf8");
for (const [a, b] of Object.entries(map)) s = s.replace(new RegExp(a, "gi"), b);
writeFileSync("src/styles.css", s);
console.log("styles.css retuned");
