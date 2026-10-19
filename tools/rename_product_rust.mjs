import { readFileSync, writeFileSync } from "node:fs";
const jobs = {
  "src-tauri/src/tray.rs": [["Aimyria", "AimyriA go"]],
  "src-tauri/src/auto_launch.rs": [["Aimyria One", "AimyriA go"]],
  "src-tauri/src/first_run.rs": [["Aimyria", "AimyriA go"]],
  "src-tauri/src/lib.rs": [
    ["=== Aimyria v", "=== AimyriA go v"],
    [".tooltip(\"Aimyria\")", ".tooltip(\"AimyriA go\")"],
    ["older Aimyria version", "older AimyriA go version"],
    ["upgrade Aimyria", "upgrade AimyriA go"],
    ["旧版本 Aimyria", "旧版本 AimyriA go"],
    ["Aimyria One's product API", "AimyriA go's product API"],
  ],
  "src-tauri/src/commands/aimyria.rs": [
    ["Aimyria One's product API", "AimyriA go's product API"],
    ["name: \"Aimyria 一键配置工具\"", "name: \"AimyriA go\""],
  ],
};
for (const [file, pairs] of Object.entries(jobs)) {
  let s = readFileSync(file, "utf8");
  let n = 0;
  for (const [a, b] of pairs) {
    const before = s.split(a).length - 1;
    s = s.split(a).join(b);
    n += before;
  }
  writeFileSync(file, s);
  console.log(file, n, "replacements");
}
