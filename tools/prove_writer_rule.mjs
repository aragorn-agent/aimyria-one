// Prove the writer rule can go red.
//
// `check_brand`'s "Every client has a tested writer" passed while five of the
// sidebar's ten apps were unconfigurable, and while Claude Desktop sat in the
// sidebar with a complete 2,000-line implementation that no one-click link would
// ever touch. It passed because it only asked whether live_writer.rs listed *any*
// client.
//
// A rewritten rule that still passes proves nothing, and this project has now been
// bitten by that three times: a palette rule scanning an unused stylesheet, a
// preset rule scanning only the Rust seed, and this one. So: break the thing, watch
// the gate go red, put it back. A rule nobody has ever seen fail is a rule whose
// verdict carries no information.

import { readFileSync, writeFileSync, copyFileSync } from "node:fs";
import { execFileSync } from "node:child_process";

const ROOT = "D:/src/cc-switch-main";
const WRITER = `${ROOT}/src-tauri/src/live_writer.rs`;
const BACKUP = "C:/Users/a/AppData/Local/Temp/opencode/live_writer.proof.bak";

function gate() {
  let out = "";
  try {
    out = execFileSync("pnpm", ["run", "check:brand"], {
      cwd: ROOT,
      encoding: "utf8",
      shell: true,
      maxBuffer: 16 * 1024 * 1024,
    });
  } catch (e) {
    out = `${e.stdout || ""}${e.stderr || ""}`;
  }
  const clean = out.replace(/\x1b\[[0-9;]*m/g, "");
  const line = clean.split(/\r?\n/).find((l) => /tested writer/.test(l)) || "(rule line not found)";
  return line.trim();
}

copyFileSync(WRITER, BACKUP);
const before = readFileSync(WRITER, "utf8");

// Remove the ClaudeDesktop entry, leaving everything else intact.
const stripped = before.replace(
  /[ \t]*ClientSpec \{\s*\r?\n[ \t]*app_type: AppType::ClaudeDesktop,\s*\r?\n[ \t]*label: "Claude Desktop",\s*\r?\n[ \t]*\/\/[^\r\n]*\r?\n[ \t]*cli: None,\s*\r?\n[ \t]*\},\s*\r?\n/,
  "",
);

if (stripped === before) {
  writeFileSync(WRITER, before);
  console.error("could not find the ClaudeDesktop entry — did not modify anything");
  process.exit(1);
}

writeFileSync(WRITER, stripped);
const broken = gate();
// BACKUP -> WRITER. The first version of this line was the other direction, which
// overwrote the good backup with the stripped file and left the tree broken — the
// exact silent-failure shape this project keeps meeting. Restore, then verify, not
// restore and assume.
copyFileSync(BACKUP, WRITER);
const restored = gate();
const after = readFileSync(WRITER, "utf8");
if (!after.includes("Claude Desktop")) {
  console.error("restore did not take — the tree is still stripped");
  process.exit(1);
}

console.log(`  with ClaudeDesktop    : ${before.match(/app_type: AppType::\w+/g).length} writers — ${before.includes("Claude Desktop") ? "present" : "absent"}`);
console.log(`  gate (ClaudeDesktop)  : ${broken}`);
console.log(`  gate (restored)       : ${restored}`);

const wentRed = /FAIL/.test(broken) || !/PASS/.test(broken);
const backGreen = /PASS/.test(restored);
console.log("");
console.log(`  rule goes red without it : ${wentRed ? "YES" : "NO — the rule is still vacuous"}`);
console.log(`  restored to green        : ${backGreen ? "YES" : "NO"}`);

if (!wentRed) process.exit(1);
