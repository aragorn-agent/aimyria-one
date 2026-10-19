import { readFileSync, writeFileSync } from "node:fs";
let s = readFileSync("src/assets/brand/aimyria-mark.svg", "utf8");
s = s.replaceAll('fill="#000000"', 'fill="#8FA9C4"');
writeFileSync("src/icons/extracted/aimyria.svg", s);
console.log("aimyria.svg", s.length);
