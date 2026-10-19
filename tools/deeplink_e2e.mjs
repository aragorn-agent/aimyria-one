// Deep-link end-to-end, run against the real exe.
//
// The product claim is: click 一键配置 in a browser, the client's config file
// on disk changes. Neither the writer's unit tests nor the parser's tests cover
// that; only the real process receiving a real `aimyria://` URL does.
//
// Steps: start the app (it must be running to receive the URL), fire the link,
// wait for the file to change, then read the file back and assert on content —
// not just on a changed hash, because a tool that overwrites the file with
// garbage has "changed" it too.
//
// Nothing here is destructive to a machine the user cares about if run with
// BACKUP=1, which copies the targets first.

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, copyFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";

const EXE = process.argv[2] ?? "D:\\Rust\\build\\aimyria-one\\release\\aimyria-one.exe";
const APP = process.argv[3] ?? "claude";
const BACKUP = process.env.BACKUP === "1";

const RELAY = "https://api.aimyria.com";
const DUMMY_KEY = "sk-e2e-dummy-key-0123456789";

/** Where each client's live config lives, and how to read the key back. */
const TARGETS = {
  claude: {
    file: join(homedir(), ".claude", "settings.json"),
    read: (text) => JSON.parse(text).env?.ANTHROPIC_AUTH_TOKEN,
    base: (text) => JSON.parse(text).env?.ANTHROPIC_BASE_URL,
  },
  codex: {
    file: join(homedir(), ".codex", "auth.json"),
    read: (text) => JSON.parse(text).OPENAI_API_KEY,
    base: () => null,
  },
};

const target = TARGETS[APP];
if (!target) {
  console.error(`unknown app: ${APP}`);
  process.exit(2);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const hash = (p) => {
  try {
    return readFileSync(p, "utf8").length;
  } catch {
    return -1;
  }
};

function report(pass, name, detail) {
  console.log(`  ${pass ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
  return pass;
}

// ── setup ─────────────────────────────────────────────────────────────────

console.log(`exe    : ${EXE}`);
console.log(`app    : ${APP}`);
console.log(`target : ${target.file}`);
console.log("");

if (BACKUP && existsSync(target.file)) {
  const bak = `${target.file}.e2e-backup`;
  copyFileSync(target.file, bak);
  console.log(`backed up to ${bak}`);
}

mkdirSync(join(homedir(), ".aimyria"), { recursive: true });

// ── start the app ─────────────────────────────────────────────────────────

console.log("starting app…");
const app = spawn(EXE, [], { detached: true, stdio: "ignore" });
app.unref();
await sleep(6000);

if (app.exitCode !== null) {
  console.error(`  app exited immediately with code ${app.exitCode}`);
  process.exit(1);
}
console.log("  app is running (a deep link needs a receiver)");
console.log("");

// ── fire ──────────────────────────────────────────────────────────────────

const before = existsSync(target.file) ? readFileSync(target.file, "utf8") : null;
const url =
  `aimyria://v1/import?resource=provider&app=${APP}&name=Aimyria` +
  `&endpoint=${RELAY}&apiKey=${DUMMY_KEY}&homepage=https://aimyria.com`;
console.log(`firing ${url}`);

const fire = spawn("cmd", ["/c", "start", "", url], { stdio: "ignore" });
await new Promise((r) => fire.on("exit", r));

// ── wait for the write ────────────────────────────────────────────────────

let after = null;
const deadline = Date.now() + 25_000;
while (Date.now() < deadline) {
  await sleep(400);
  if (!existsSync(target.file)) continue;
  const text = readFileSync(target.file, "utf8");
  if (text !== before) {
    after = text;
    break;
  }
}

if (after === null) {
  console.error("");
  console.error("  FAIL  the target file did not change within 25s");
  console.error("");
  console.error("  Possible causes:");
  console.error("   1. the app was not running (deep links need a receiver)");
  console.error("   2. aimyria:// is registered to a different build:");
  console.error("      HKCU\\Software\\Classes\\aimyria\\shell\\open\\command");
  console.error("   3. the parse failed — check:");
  console.error(`      ${join(homedir(), ".aimyria", "logs", "aimyria-one.log")}`);
  process.exitCode = 1;
} else {
  console.log("");
  let ok = true;
  ok &= report(target.read(after) === DUMMY_KEY, "api key written", target.read(after));
  if (target.base) {
    ok &= report(target.base(after) === RELAY, "base url written", String(target.base(after)));
  }
  ok &= report(true, "file length", `${before?.length ?? 0} -> ${after.length}`);
  console.log("");
  console.log(ok ? "deep link end-to-end: OK" : "deep link end-to-end: FAILED");
  process.exitCode = ok ? 0 : 1;
}

// ── cleanup ───────────────────────────────────────────────────────────────

try {
  process.kill(-app.pid);
} catch {
  /* already gone */
}
rmSync(`${join(homedir(), ".aimyria", "logs", "aimyria-one.log")}.e2e-tmp`, { force: true });