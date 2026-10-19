import { readFileSync, appendFileSync } from "node:fs";
const add = `
// Tray state icons: black before a relay is configured, green after.
for (const [size, suffix] of [[16, "16"], [32, "32"], [48, "48"]]) {
  const pad = Math.max(1, Math.round(size * 0.12));
  const radius = Math.round(size * 0.22);
  render(
    iconSvg(size, { pad, color: "#FFFFFF", bg: "#000000", radius }),
    `src-tauri/icons/tray/windows/tray-idle-\${suffix}.png`,
    size,
  );
  render(
    iconSvg(size, { pad, color: "#FFFFFF", bg: "#22A06B", radius }),
    `src-tauri/icons/tray/windows/tray-connected-\${suffix}.png`,
    size,
  );
}
`;
appendFileSync("tools/render_icons.mjs", add);
console.log("appended");
