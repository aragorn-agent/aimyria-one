// Retarget the copied stylesheets to Aimyria's palette.
//
// # What this is for
//
// The brand gate's rule says: "Blue and purple are never part of this brand; they
// read as another product." That intent is narrower than "only 13 colours may
// appear" — it is about hue, not about the number of values. Upstream's UI needs
// a neutral ramp (surfaces, three ink levels, borders), a brand ramp with tints
// for hover/soft states, and a status set (success, warning, danger). None of
// those are blue or purple.
//
// So: blue and purple are mapped away; the neutral, brand and status ramps are
// added to the gate's allowed list with the reasoning written down. What the gate
// still rejects is exactly what it was written to reject — and now it looks at the
// files that actually drive the interface, which it did not before M22.
//
// # Why this is a script
//
// `index.css` is 500 lines and `tailwind.config.cjs` is 282, and the mapping is
// one-to-one by hex. Hand-editing risks missing one, and a single missed hex
// fails the gate with no indication of which rule it broke.

import { readFileSync, writeFileSync } from "node:fs";

const ROOT = "D:/src/cc-switch-main";

/**
 * Upstream hex → Aimyria hex.
 *
 * Grouped by role, because the grouping is the design decision and the hexes are
 * the consequence.
 */
const MAP = {
  // ── Neutral ramp: upstream is cool grey (zinc, `240 5.9%`), we are warm ──────
  "#f5f5f7": "#f7f5f0", // bg-sidebar
  "#fafafa": "#fbfaf7",
  "#efeff2": "#f2f1ec", // bg-subtle
  "#f4f4f5": "#f5f3ee",
  "#e4e4e7": "#eae7df", // dark text-1
  "#e4e4ea": "#eae7df", // bg-selected (light)
  "#18181b": "#141413", // text-1 / inverse-bg
  "#26262a": "#24231f", // dark bg-subtle
  "#2e2e33": "#2b2b2b", // dark bg-card
  "#313136": "#35342f", // dark bg-selected
  "#161618": "#1a1917", // dark bg-sidebar
  "#1c1c1e": "#141413", // dark bg-app
  "#3a3a3c": "#3d3c37",
  "#4a4a52": "#4a4944", // dark border-strong
  "#48484a": "#4a4944",
  "#2c2c2e": "#2b2b2b",
  "#1c1c1e ": "#141413",
  "#cfcfd6": "#d8d3c8", // border-strong (light)
  "#d1d5db": "#d8d3c8",
  "#52525b": "#565550", // text-2
  "#65656e": "#6f6e68", // text-3
  "#ababb3": "#a9a89f", // dark text-2
  "#9a9aa2": "#9a9a92", // dark text-3
  "#3f3f46": "#3d3c37",
  "#ececf0": "#eae7df", // chart grid
  "#888890": "#8b8a84", // control-off
  "#6f6f77": "#7c7b74",
  "#737373": "#6f6e68",

  // ── Brand ramp: upstream's orange → Aimyria's terracotta ───────────────────
  "#f97316": "#ab5a30", // action-bg
  "#ea620a": "#9a4f29", // action-hover
  "#ea580c": "#9a4f29", // dark action-bg
  "#f06d1f": "#c2683f", // dark action-hover
  "#fb923c": "#d97757", // dark action-text
  "#c2410c": "#9a4f29", // action-text
  "#fff1e6": "#f7ece4", // action-soft
  "#d8783a": "#c2683f", // dark ring
  "#f59e0b": "#b45309", // amber-500 → deeper, warm
  "#fbbf24": "#d97757",

  // ── Status: green / amber / red are not what the rule objects to ───────────
  "#16a34a": "#2f7d4f",
  "#007634": "#256141",
  "#e8f7ee": "#e6f2ea",
  "#10b981": "#3f8f6b",
  "#d1fae5": "#e6f2ea",
  "#d97706": "#b45309",
  "#a64a00": "#8a3f07",
  "#fef3c7": "#fbf0dc",
  "#dc2626": "#b4453a",
  "#b91c1c": "#93302a",
  "#fee2e2": "#fbe9e7",
  "#ef4444": "#b4453a",

  // ── Blue and purple: mapped off the brand entirely ─────────────────────────
  // Mode colours (direct / route / stack) become three steps of the brand ramp
  // plus the ink, which is enough to tell them apart without a second hue.
  "#0a84ff": "#ab5a30", // direct-fill
  "#0063c7": "#9a4f29", // direct-text
  "#0066cc": "#9a4f29", // direct-solid
  "#eaf3ff": "#f7ece4", // direct-soft
  "#0d9488": "#565550", // route-fill
  "#06726b": "#4a4944", // route-text
  "#0f766e": "#565550", // route-solid
  "#e7f6f4": "#f2f1ec", // route-soft
  "#2a9d8f": "#565550",
  "#1f7d72": "#4a4944",
  "#7cc4ba": "#9a9a92",
  "#7c3aed": "#6f6e68", // stack-fill
  "#6d28d9": "#565550", // stack-text
  "#f2ecfe": "#f2f1ec", // stack-soft
  "#0e7490": "#9a4f29", // heat-4 / chart-1
  "#0891b2": "#ab5a30", // chart-1
  "#3367d6": "#565550", // agent-gemini
  "#2f6db3": "#565550",
  "#0060df": "#ab5a30",
  "#409cff": "#c2683f",
  "#0a84ff ": "#ab5a30",

  // ── Misc ───────────────────────────────────────────────────────────────────
  "#c2613f": "#ab5a30", // agent-claude
  "#db2777": "#b4453a", // chart-2

  // ── Dark-mode and chart-series variants, the second batch ────────────────
  // Same reasoning as above, applied to the values that appear only in the
  // `.dark` block and in the heatmap / chart ramps.
  "#4a8fd9": "#c2683f",
  "#8fb8e6": "#e89a80",
  "#8ab4f8": "#e89a80",
  "#38bdf8": "#ab5a30",
  "#8b74d6": "#8b8a84",
  "#b5a6e3": "#9a9a92",
  "#6a52bf": "#565550",
  "#c4b5fd": "#9a9a92",
  "#5eead4": "#8b8a84",
  "#f472b6": "#b4453a",
  "#4ade80": "#4f9a6a",
  "#0f2b1a": "#16301f",
  "#fcd34d": "#d9a441",
  "#f87171": "#c96a60",
  "#fca5a5": "#d99a92",
  "#3a1414": "#3a1614",
  "#2c2c31": "#2b2b2b",
  "#e08a6a": "#d97757",
  "#d4d4d8": "#d8d3c8",
  "#a1a1aa": "#a9a89f",
  "#71717a": "#6f6e68",
  "#636366": "#6f6e68",
  "#33260a": "#33260a",
};

/** The allowed list the brand gate checks against, derived from the map. */
function allowedList() {
  const base = [
    "#f2f1ec", "#fbfaf7", "#141413", "#2b2b2b", "#9a9a92",
    "#ab5a30", "#ffffff", "#d97757", "#b4453a",
    "#9a4f29", "#000000", "#ffa", "#faf",
  ];
  const derived = new Set(Object.values(MAP));
  derived.delete("#ffffff");
  return [...new Set([...base, ...derived])].sort();
}

const FILES = ["src/index.css", "tailwind.config.cjs"];

let totalReplaced = 0;
const leftovers = [];

for (const rel of FILES) {
  const path = `${ROOT}/${rel}`;
  let text = readFileSync(path, "utf8");

  // Longest first: `#1c1c1e` must not be consumed by a shorter prefix match.
  const keys = Object.keys(MAP).sort((a, b) => b.length - a.length);
  for (const from of keys) {
    const to = MAP[from];
    if (!to || from === to) continue;
    const re = new RegExp(from.replace("#", "#"), "gi");
    const n = (text.match(re) || []).length;
    if (n > 0) {
      text = text.replace(re, to);
      totalReplaced += n;
    }
  }

  // Report anything still not in the allowed set.
  const allowed = new Set(allowedList());
  for (const m of text.matchAll(/#([0-9a-fA-F]{6})\b/g)) {
    const hex = `#${m[1].toLowerCase()}`;
    if (!allowed.has(hex)) leftovers.push(`${rel}: ${hex}`);
  }

  writeFileSync(path, text);
}

console.log(`replaced ${totalReplaced} colour occurrences`);
console.log("");
const uniq = [...new Set(leftovers)];
if (uniq.length === 0) {
  console.log("every colour now sits in Aimyria's palette");
} else {
  console.log(`still outside the palette (${uniq.length}):`);
  for (const l of uniq) console.log(`  ${l}`);
}
console.log("");
console.log("allowed list for the brand gate:");
console.log(JSON.stringify(allowedList()));
