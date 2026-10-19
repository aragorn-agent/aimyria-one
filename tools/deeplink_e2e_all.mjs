// The product promise, tested literally: one click in a browser, every client
// configured.
//
// This used to fire one deep link per client and assert that client. That is no
// longer what the software does — a link applies to every client — so the test
// would have kept passing while measuring something that no longer exists.
//
// The whole point is that a user should not have to know which CLI they use, so
// the assertion is now: fire once, check five.
//
// Both start paths are covered. The cold one is not an edge case: this app
// starts with Windows and hides its window, so "not running yet" is the normal
// state on a freshly installed machine and right after a sign-in.

import { spawn, spawnSync } from "node:child_process";
import { existsSync, readFileSync, copyFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";

const EXE = process.argv[2] ?? "D:\\Rust\\build\\aimyria-one\\release\\aimyria-one.exe";
const RELAY = "https://api.aimyria.com";
const KEY = "sk-e2e-oneclick-0123456789";

/**
 * Each client: the file that must change, how to read the key back, and how to
 * check the base URL.
 *
 * OpenClaw's config is JSON5-style — unquoted keys, trailing commas — and has
 * to stay that way, because the CLI parses it that way. So its two values are
 * pulled out with a regex rather than JSON.parse, which would reject valid
 * config.
 */
const CLIENTS = [
  {
    app: "claude",
    label: "Claude Code",
    file: join(homedir(), ".claude", "settings.json"),
    key: (t) => JSON.parse(t).env?.ANTHROPIC_AUTH_TOKEN,
    base: (t) => JSON.parse(t).env?.ANTHROPIC_BASE_URL,
  },
  {
    app: "codex",
    label: "Codex",
    // Codex's credential location depends on which writer ran. Older builds wrote
    // ~/.codex/auth.json with OPENAI_API_KEY; the copied-upstream writer puts the
    // bearer token in ~/.codex/config.toml as `experimental_bearer_token`, with
    // base_url on the line above it.
    //
    // Asserting only auth.json was what made this test fail for a milestone while
    // the app was, in fact, configuring Codex correctly — the log said
    // "codex 切到 aimyria 完成". So: look in both places, and require the key in
    // whichever one exists. A missing base_url is acceptable for auth.json, where
    // it has never lived.
    file: join(homedir(), ".codex", "config.toml"),
    alt: [{ file: join(homedir(), ".codex", "auth.json"), json: true }],
    key: (t) => /experimental_bearer_token\s*=\s*"([^"]*)"/.exec(t)?.[1],
    base: (t) => /base_url\s*=\s*"([^"]*)"/.exec(t)?.[1],
  },
  {
    app: "gemini",
    label: "Gemini CLI",
    file: join(homedir(), ".gemini", ".env"),
    key: (t) => /^GEMINI_API_KEY=(.*)$/m.exec(t)?.[1],
    base: (t) => /^GOOGLE_GEMINI_BASE_URL=(.*)$/m.exec(t)?.[1],
  },
  {
    app: "opencode",
    label: "OpenCode",
    file: join(homedir(), ".config", "opencode", "opencode.json"),
    key: (t) => JSON.parse(t).provider?.aimyria?.options?.apiKey,
    base: (t) => JSON.parse(t).provider?.aimyria?.options?.baseURL,
    // OpenCode alone refuses a provider with an empty `npm` or `models` — a new ID
    // cannot inherit an existing definition — so a key and a base URL are not
    // enough to call this client configured. Assert the two fields the writer
    // refuses to omit, or the test passes on a file the app itself rejects.
    extra: (t) => {
      const p = JSON.parse(t).provider?.aimyria ?? {};
      const models = Object.keys(p.models ?? {});
      if (!p.npm) return `npm missing`;
      if (models.length === 0) return `models empty`;
      return null;
    },
    detail: (t) => {
      const p = JSON.parse(t).provider?.aimyria ?? {};
      return `${p.npm} / ${Object.keys(p.models ?? {}).join(",")}`;
    },
  },
  {
    app: "openclaw",
    label: "OpenClaw",
    file: join(homedir(), ".openclaw", "openclaw.json"),
    key: (t) => /"apiKey"\s*:\s*"([^"]*)"/.exec(t)?.[1],
    base: (t) => /"baseUrl"\s*:\s*"([^"]*)"/.exec(t)?.[1],
  },
];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const readIf = (p) => (existsSync(p) ? readFileSync(p, "utf8") : null);
const backups = [];

function backup(path) {
  if (!backups.some((b) => b.path === path) && existsSync(path)) {
    const dest = `${path}.e2e-bak`;
    copyFileSync(path, dest);
    backups.push({ path, dest });
  }
}
function restoreAll() {
  for (const { path, dest } of backups.reverse()) {
    try {
      copyFileSync(dest, path);
      rmSync(dest, { force: true });
    } catch {
      console.error(`  !! could not restore ${path}`);
    }
  }
}
function ok(pass, name, detail) {
  console.log(`      ${pass ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
  return pass;
}

// ── one pass ──────────────────────────────────────────────────────────────

/**
 * Fire a single deep link with the app in the given state, then assert every
 * client ended up pointing at the relay.
 */
async function runPass(label, coldStart) {
  console.log(`\n══ ${label} ══`);

  const before = {};
  for (const c of CLIENTS) {
    before[c.app] = readIf(c.file);
    backup(c.file);
    // Clear the target so "the file appeared" is itself a real signal, and so a
    // stale value cannot pass as a fresh write.
    rmSync(c.file, { force: true });
  }

  let app = null;
  if (!coldStart) {
    app = spawn(EXE, [], { detached: true, stdio: "ignore" });
    app.unref();
    await sleep(6000);
    if (app.exitCode !== null) {
      console.error(`  app exited immediately (code ${app.exitCode})`);
      return false;
    }
  }

  // One click. `app=claude` because the console page had that client selected —
  // it is not a request to configure only that one.
  const url =
    `aimyria://v1/import?resource=provider&app=claude&name=Aimyria` +
    `&endpoint=${RELAY}&apiKey=${KEY}&homepage=https://aimyria.com`;

  // `rundll32 url.dll,FileProtocolHandler` is the Windows shell's own protocol
  // dispatch path, so this exercises the registry entry rather than our argv.
  //
  // `cmd /c start` looks like the obvious equivalent and is not: the shell
  // splits the URL on `&` before `start` ever sees it, so every query parameter
  // after the first runs as a separate command. Silent, and it looks like the
  // app is broken.
  spawnSync("rundll32.exe", ["url.dll,FileProtocolHandler", url], { stdio: "ignore" });

  // The handoff is asynchronous — a second process forwards argv and exits — and
  // a cold start has to boot the app first. Five clients are written in one
  // pass, so this is a short wait in the common case, not a 25s one.
  const deadline = Date.now() + 40_000;
  while (Date.now() < deadline) {
    await sleep(400);
    if (CLIENTS.every((c) => readIf(c.file) !== null && readIf(c.file) !== before[c.app])) {
      break;
    }
  }

  if (app) {
    try {
      process.kill(-app.pid);
    } catch {
      /* already gone */
    }
    await sleep(1000);
  }

  let pass = true;
  for (const c of CLIENTS) {
    // A client whose writer moved its credential file is described by `alt`: try
    // the primary path first, then each alternate, and assert against the one that
    // exists. Without this, a correct write to the new location reads as a failure
    // because the test is looking at the old one.
    const candidates = [{ file: c.file }, ...(c.alt ?? [])];
    const found = candidates.map((a) => ({ ...a, text: readIf(a.file) })).find((a) => a.text !== null);

    if (!found) {
      pass = ok(false, `${c.label} 文件未写入`, candidates.map((a) => a.file).join(" 或 "));
      continue;
    }
    const text = found.text;
    try {
      pass = ok(c.key(text) === KEY, `${c.label} api key`, String(c.key(text))) && pass;
      if (c.base) {
        pass = ok(c.base(text) === RELAY, `${c.label} base url`, String(c.base(text))) && pass;
      }
      if (c.extra) {
        const problem = c.extra(text);
        pass = ok(!problem, `${c.label} npm + models`, problem ?? c.detail(text)) && pass;
      }
    } catch (e) {
      pass = ok(false, `${c.label} 内容可解析`, String(e.message).slice(0, 60)) && pass;
    }
  }
  return pass;
}

// ── main ──────────────────────────────────────────────────────────────────

console.log(`exe: ${EXE}\n`);

// Warm first: cheaper, and it was what the original test covered, so a
// regression there is unambiguous.
const warm = await runPass("app 已在运行（单实例交接）", false);
const cold = await runPass("app 未运行（协议启动）", true);

restoreAll();
console.log("");
const allPass = warm && cold;
console.log(allPass
  ? "ONE CLICK → 5/5 CLIENTS CONFIGURED (both start paths)"
  : (warm ? "cold-start pass FAILED" : "warm pass FAILED"));
process.exitCode = allPass ? 0 : 1;