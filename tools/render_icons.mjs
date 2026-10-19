// Render the owner-approved AIMYRIA mark into every icon slot the app and the
// installer use.
//
// The mark is the traced vector from 00-logo-final, not a redraw: the previous
// icons were still upstream's. Composition follows the VI: black rounded-square
// app icon with the white mark, ochre tray glyph (visible on both light and dark
// Windows taskbars), black template glyph for macOS.
//
// Usage: node tools/render_icons.mjs

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { Resvg } from "@resvg/resvg-js";

const MARK = readFileSync("src/assets/brand/aimyria-mark.svg", "utf8");
const inner = MARK.replace(/^[\s\S]*?<svg[^>]*>/, "").replace(/<\/svg>\s*$/, "");
const VB = { x: 177.062, y: -0.526, w: 832.228, h: 699.535 };

function markGroup(size, pad, color) {
  const avail = size - pad * 2;
  const s = avail / VB.w;
  const h = VB.h * s;
  const tx = pad;
  const ty = pad + (avail - h) / 2;
  const body = inner.replace(/fill="#000000"/g, `fill="${color}"`);
  return `<g transform="translate(${tx} ${ty}) scale(${s}) translate(${-VB.x} ${-VB.y})">${body}</g>`;
}

function iconSvg(size, { pad = 0, color = "#FFFFFF", bg = null, radius = 0 }) {
  const back =
    bg === null
      ? ""
      : `<rect width="${size}" height="${size}" rx="${radius}" fill="${bg}"/>`;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${size} ${size}">${back}${markGroup(size, pad, color)}</svg>`;
}

function render(svg, out, size) {
  const png = new Resvg(svg, {
    fitTo: { mode: "width", value: size },
    background: "rgba(0,0,0,0)",
  })
    .render()
    .asPng();
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, png);
  console.log(out, png.length, "bytes");
}

// App icon: black rounded square, white mark, generous clear space.
for (const size of [32, 48, 64, 128, 256, 512, 1024]) {
  render(
    iconSvg(1024, { pad: 168, color: "#FFFFFF", bg: "#000000", radius: 228 }),
    `src-tauri/icons/app-${size}.png`,
    size,
  );
}

// The filenames Tauri/Windows expect.
const appIcon = iconSvg(1024, {
  pad: 168,
  color: "#FFFFFF",
  bg: "#000000",
  radius: 228,
});
const pairs = [
  [32, "32x32.png"],
  [128, "128x128.png"],
  [256, "128x128@2x.png"],
  [256, "256x256.png"],
  [512, "512x512.png"],
  [512, "icon.png"],
];
for (const [size, name] of pairs) {
  render(appIcon, `src-tauri/icons/${name}`, size);
}

// Windows tray: ochre glyph, transparent background.
for (const [size, name] of [
  [16, "tray-16.png"],
  [32, "tray-32.png"],
  [48, "tray-48.png"],
]) {
  render(
    iconSvg(size, { pad: 1, color: "#C27849" }),
    `src-tauri/icons/tray/windows/${name}`,
    size,
  );
}

// macOS status bar: template images are black + alpha; the OS tints them.
for (const [size, name] of [
  [22, "statusTemplate.png"],
  [44, "statusTemplate@2x.png"],
  [66, "statusbar_template_3x.png"],
]) {
  render(
    iconSvg(size, { pad: 1, color: "#000000" }),
    `src-tauri/icons/tray/macos/${name}`,
    size,
  );
}

// Tray state icons: the mark alone, no tile background.
// Idle = black mark; connected = WeChat green #07C160. Transparent background
// so the tray shows the logo, not a square.
for (const size of [16, 32, 48]) {
  const pad = Math.max(1, Math.round(size * 0.08));
  render(
    iconSvg(size, { pad, color: "#000000" }),
    `src-tauri/icons/tray/windows/tray-idle-${size}.png`,
    size,
  );
  render(
    iconSvg(size, { pad, color: "#07C160" }),
    `src-tauri/icons/tray/windows/tray-connected-${size}.png`,
    size,
  );
}

// Keep the renderer's bundled copy in sync (used by the in-app About/hero).
render(appIcon, "src/assets/icons/app-icon.png", 256);
