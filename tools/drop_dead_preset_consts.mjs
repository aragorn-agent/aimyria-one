// Delete the preset-only declarations that became unreferenced when the arrays emptied.
//
// `noUnusedLocals` / `noUnusedParameters` treat a module-local name nothing reads
// as an error, so emptying the preset arrays left ~20 of upstream's helper
// declarations dead: instruction blocks, model catalogues, compat flags, and two
// builder functions whose only callers were the removed preset entries.
//
// They are dead by definition, and each one serves a relay this build does not
// offer — so deleting is the honest direction, not a workaround.
//
// Four shapes have to be handled, and the first version of this only did one:
//
//   const X = ...;            single or multi-line, up to the line that balances it
//   function f(...) { ... }   up to the matching `}` at the same indent
//   import { X } from "..."   remove just the name, keep the rest of the import
//   const X = { ... } as const;  same as the first
//
// Exported names are left alone: tsc has said nothing *here* reads them, which is
// not the same as nothing anywhere does.

import { readFileSync, writeFileSync } from "node:fs";

const FILES = {
  "D:/src/cc-switch-main/src/config/grokBuildProviderPresets.ts": [
    "GROK_BUILD_DEFAULT_MODEL",
  ],
  "D:/src/cc-switch-main/src/config/piProviderPresets.ts": ["piModel"],
};

const balance = (s) =>
  (s.match(/[[{(]/g) || []).length - (s.match(/[\])}]/g) || []).length;

let removed = 0;
const kept = [];

for (const [file, names] of Object.entries(FILES)) {
  const lines = readFileSync(file, "utf8").split("\n");
  const out = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];

    // `import { a, b, X } from "..."`, single-line or multi-line. The multi-line
    // form is why the first pass left `piModel` in place: the specifiers sit on
    // their own lines, so a `/^import \{.*\} from/` on one line never matches.
    if (/^\s*import\s*\{/.test(line) && !/\}\s*from/.test(line)) {
      const specs = [];
      let j = i + 1;
      let close = -1;
      while (j < lines.length) {
        const m = lines[j].match(/^\s*\}\s*(?:from\s*["'][^"']+["'])?;?\s*$/);
        if (m) {
          close = j;
          break;
        }
        specs.push(lines[j]);
        j += 1;
      }
      if (close !== -1) {
        const survivors = [];
        let dropped = 0;
        for (const s of specs) {
          const trimmed = s.trim().replace(/,$/, "");
          const bare = trimmed.replace(/^type\s+/, "").split(/\s+as\s+/)[0];
          if (names.includes(bare)) {
            dropped += 1;
          } else {
            survivors.push(s);
          }
        }
        if (dropped > 0) {
          removed += dropped;
          if (survivors.length > 0) {
            const from = lines[close].match(/from\s*["'][^"']+["']/);
            out.push(lines[i]);
            out.push(...survivors);
            out.push(`}${from ? ` ${from[0]}` : ""};`);
          }
          i = close + 1;
          continue;
        }
      }
      out.push(line);
      i += 1;
      continue;
    }

    const imp = line.match(/^(\s*import\s*\{)([\s\S]*?)(\},?\s*from\s*["'][^"']+["'];?)\s*$/);
    if (imp) {
      const specs = imp[2]
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
      const survivors = [];
      let dropped = 0;
      for (const s of specs) {
        const bare = s.replace(/^type\s+/, "").split(/\s+as\s+/)[0];
        if (names.includes(bare)) {
          dropped += 1;
        } else {
          survivors.push(s);
        }
      }
      if (dropped > 0) {
        removed += dropped;
        if (survivors.length > 0) {
          out.push(`${imp[1]} ${survivors.join(", ")} ${imp[3]}`);
        }
        // Survivors empty: drop the whole import.
        i += 1;
        continue;
      }
      out.push(line);
      i += 1;
      continue;
    }

    // `function f(...)` or `const X =`, at this indent.
    const decl = line.match(/^(\s*)(?:export\s+)?(function\s+(\w+)|const\s+(\w+))\s*(\(|[:=])/);
    if (decl) {
      const name = decl[3] || decl[4];
      if (!names.includes(name)) {
        out.push(line);
        i += 1;
        continue;
      }
      if (/^export\s/.test(line.trim())) {
        kept.push(`${file.split("/").pop()}: ${name} (exported — left alone)`);
        out.push(line);
        i += 1;
        continue;
      }

      // Consume to the end of the statement. Brackets balance for both forms;
      // a one-line declaration is already balanced on its own.
      let depth = balance(line);
      let j = i + 1;
      while (depth > 0 && j < lines.length) {
        depth += balance(lines[j]);
        j += 1;
      }
      // A trailing semicolon on its own line (multi-line object literals).
      while (j < lines.length && lines[j].trim() === ";") j += 1;
      // Swallow one blank line so removal does not leave a gap.
      if (j < lines.length && lines[j].trim() === "") j += 1;
      i = j;
      removed += 1;
      continue;
    }

    out.push(line);
    i += 1;
  }

  writeFileSync(file, out.join("\n"));
}

console.log(`removed ${removed} declaration(s)`);
if (kept.length) {
  console.log("");
  console.log("left alone:");
  for (const k of kept) console.log(`  ${k}`);
}
