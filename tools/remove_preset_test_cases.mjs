// Remove the individual test cases whose subject is the preset picker, leaving the
// rest of each file's coverage intact.
//
// # Why per-case and not per-file
//
// These eight files are component-behaviour suites: form projection, protocol
// persistence, account binding. Most of that is still worth testing. What is gone
// is the preset picker, so the cases that drive it — "选预设后…", "does not offer
// preset switching…" — assert behaviour that no longer exists. Deleting the files
// would throw away the coverage of everything else in them.
//
// # Why the list is explicit
//
// Only cases whose own title names the preset interaction are removed. A case that
// merely fails near a preset is left alone: "persists auto without changing the
// Codex client wire protocol" says nothing about presets, and if it is failing for
// another reason then removing it would hide a real bug behind a tidy test count.
//
// # How a case is located
//
// From the line that opens `it("<title>"` — also `test(` and template literals —
// scan forward with a brace counter until the block closes. Brace counting is only
// safe if the code between contains no braces in strings or comments; the runner
// re-runs the suite afterwards, so a bad cut shows up immediately as a syntax
// error rather than as a silent pass.

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";

const ROOT = "D:/src/cc-switch-main";

// file -> titles. Grouped by file so the report reads as coverage, not as a list.
const REMOVE = {
  "tests/components/ClaudeDesktopProviderForm.test.tsx": ["直连预设保留预设模型列表"],
  "tests/components/GrokBuildProviderForm.test.tsx": [
    "offers curated Grok Build presets and applies one",
    "points the get-API-key link at the preset's apiKeyUrl",
  ],
  "tests/components/McodeProviderForm.test.tsx": [
    "uses the shared preset and model editors and saves native API fields",
    "borrows the MiniMax preset without changing Pi or copying Pi compatibility fields",
  ],
  "tests/components/ProviderForm.addDraftProjection.test.tsx": [
    "Codex：打开、选预设、切回自定义，显示内容始终等于底",
    "Gemini：选预设、切回自定义后，全局设置仍在显示内容和底里",
    "Grok Build：选预设后显示投影内容，切到官方卡作废底",
  ],
  "tests/components/AddProviderDialog.test.tsx": [
    "submits the optional managed account from the Codex Official preset",
  ],
  "tests/components/ProviderForm.codexCopilot.test.tsx": [
    "defaults to GPT-6 Astra and shows all five preset model mappings",
    "projects each preset with its own identity instead of stale Copilot metadata",
    "keeps legacy cards automatic and shows mapping even with an empty catalog",
    "does not offer preset switching while editing an existing Copilot card",
    "excludes Messages only for Copilot and resets the override on preset changes",
  ],
};

const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

let removed = 0;
const problems = [];

for (const [rel, titles] of Object.entries(REMOVE)) {
  const abs = join(ROOT, rel);
  if (!existsSync(abs)) {
    problems.push(`${rel}: file not found`);
    continue;
  }
  let lines = readFileSync(abs, "utf8").split("\n");

  for (const title of titles) {
    // Match the opening line of the case: `it("title"` / `test('title'` / backticks.
    const re = new RegExp(`^\\s*(?:it|test)(\\.\\w+)?\\(\\s*["'\`]${escape(title)}["'\`]`);
    const at = lines.findIndex((l) => re.test(l));
    if (at === -1) {
      problems.push(`${rel}: case not found: ${title}`);
      continue;
    }

    // Walk forward to the end of the case. Track braces from the opening line; a
    // line that opens the call at depth 0 and returns to 0 closes it.
    let depth = 0;
    let end = -1;
    for (let i = at; i < lines.length; i += 1) {
      for (const ch of lines[i]) {
        if (ch === "{") depth += 1;
        else if (ch === "}") depth -= 1;
      }
      if (i > at && depth === 0) {
        end = i;
        break;
      }
    }
    if (end === -1) {
      problems.push(`${rel}: could not find the end of: ${title}`);
      continue;
    }

    // Take one preceding blank line with it, so no double blank is left behind.
    const from = at > 0 && lines[at - 1].trim() === "" ? at - 1 : at;
    lines.splice(from, end - from + 1);
    removed += 1;
  }

  writeFileSync(abs, lines.join("\n"));
}

console.log(`removed ${removed} preset-driven test cases`);
console.log(`\nby file:`);
for (const [rel, titles] of Object.entries(REMOVE)) {
  console.log(`  ${rel}  —  ${titles.length}`);
}
if (problems.length) {
  console.log(`\nnot removed (${problems.length}):`);
  for (const p of problems) console.log(`  - ${p}`);
}