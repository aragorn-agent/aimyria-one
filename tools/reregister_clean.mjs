// Remove the empty registration comment blocks, then re-register cleanly.
//
// # What happened
//
// M25: register_commands.mjs inserted 115 commands as
//      `...["a", "b"],` (quoted, spread). fix_command_quoting.mjs unquoted them
//      into one-per-line paths. That worked.
//
// M26: after the proxy copy, the script ran again, found 115 "missing" again and
//      inserted a SECOND block in the quoted-spread form. drop_stub_spread.mjs then
//      removed the line matching `^...\[...\],\]\);$` — and that line was the whole
//      command list.
//
// Result: three copies of the explanation comment in lib.rs, and zero commands
// between them. `cargo check` stayed green the whole time, because removing
// entries from the handler list is never an error — it just makes the app fail at
// runtime with "Command X not found", which is what the Codex routing dialog did.
//
// This deletes the empty blocks and re-runs the (now fixed) register script.

import { readFileSync, writeFileSync, unlinkSync } from "node:fs";
import { execFileSync } from "node:child_process";

const LIB = "D:/src/cc-switch-main/src-tauri/src/lib.rs";

let text = readFileSync(LIB, "utf8");

// A stub block: the header comment, its explanation, and nothing else — the next
// non-comment line is `]);` or a real entry.
const STUB =
  /\n *\/\/ ── Commands that were defined but never registered ─+\n(?: *\/\/[^\n]*\n)*/g;

const before = text.length;
text = text.replace(STUB, "\n");
const removedChars = before - text.length;

if (removedChars === 0) {
  console.log("no stub blocks found");
} else {
  writeFileSync(LIB, text);
  console.log(`removed ${removedChars} chars of empty comment blocks`);
}

// Re-register, now that the script emits bare `commands::name` paths.
const out = execFileSync(
  "node",
  ["D:/src/cc-switch-main/tools/register_commands.mjs"],
  { encoding: "utf8", env: { ...process.env, PATH: `C:\\Program Files\\nodejs;${process.env.PATH}` } },
);
process.stdout.write(out);

// Verify the result, because the failure mode above was invisible to the compiler.
const after = readFileSync(LIB, "utf8");
const names = [...after.matchAll(/commands::([a-z0-9_]+)/g)].map((m) => m[1]);
const dupes = [...new Set(names.filter((v, i) => names.indexOf(v) !== i))];
console.log("");
console.log(`commands:: entries : ${names.length}`);
console.log(`duplicates         : ${dupes.length}${dupes.length ? " -> " + dupes.join(", ") : ""}`);
const stubs = [...after.matchAll(STUB)].length;
console.log(`stub blocks left   : ${stubs}`);

const mustHave = [
  "set_proxy_takeover_for_app",
  "set_proxy_route",
  "start_proxy_server",
  "get_app_mode",
  "get_proxy_status",
  "add_to_failover_queue",
  "list_sessions",
];
const missingNow = mustHave.filter((c) => !names.includes(c));
if (missingNow.length) {
  console.error("still missing: " + missingNow.join(", "));
  process.exit(1);
}
console.log("spot-check: all seven previously-broken commands are present");
