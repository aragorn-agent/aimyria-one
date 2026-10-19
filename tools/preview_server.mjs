// A preview harness for the built renderer.
//
// The Aimyria renderer is a Tauri app: every screen reads through
// `invoke()`, which only exists inside the WebView. That makes the real UI hard
// to inspect without the app, a focusable window, and a way to click it — and on
// this desktop other apps keep stealing the foreground, so screenshots of the
// live window have repeatedly come back showing the wrong window or a blank
// frame.
//
// This serves `dist/` unchanged and adds ONE extra page, `__preview.html`, that
// is `index.html` plus a `window.__TAURI_INTERNALS__.invoke` shim answering with
// fixtures. The production `index.html` is never modified, so the release build
// and the brand gate see exactly what they saw before.
//
// What this proves: the components mount, layout and CSS are right, and each
// screen renders the state the backend would return.
// What it does not prove: that the Rust commands return those states. That is
// covered by `cargo check`, and end to end by the deep-link regression.

import { createServer } from "node:http";
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { join, extname } from "node:path";

const DIST = process.argv[2] || "D:/src/cc-switch-main/dist";
const PORT = Number(process.argv[3] || 8801);

/**
 * Fixtures keyed by command name.
 *
 * Shapes come from the real serde structs — `McpApps` for MCP, `SkillApps` for
 * skills, `Prompt` for prompts. A preview that fed a convenient shape would
 * prove nothing about a component that has to survive the real one.
 */
const now = Math.floor(Date.now() / 1000);

const FIXTURES = {
  // Field names here are the real ones. `ClientStatus` carries `label`, not
  // `name` — a fixture that guessed wrong crashes the renderer before it paints,
  // which is a worse failure than an empty screen because it looks like the app
  // is broken rather than the harness.
  get_client_status: [
    { app: "claude", label: "Claude Code", supported: true, cliInstalled: true, livePath: "C:/Users/a/.claude/settings.json", configured: true, currentProvider: "aimyria" },
    { app: "codex", label: "Codex", supported: true, cliInstalled: true, livePath: "C:/Users/a/.codex/config.toml", configured: true, currentProvider: "aimyria" },
    { app: "gemini", label: "Gemini CLI", supported: true, cliInstalled: false, livePath: "C:/Users/a/.gemini/.env", configured: true, currentProvider: "aimyria" },
    { app: "opencode", label: "OpenCode", supported: true, cliInstalled: true, livePath: "C:/Users/a/.config/opencode/opencode.json", configured: true, currentProvider: "aimyria" },
    { app: "openclaw", label: "OpenClaw", supported: true, cliInstalled: false, livePath: "C:/Users/a/.openclaw/openclaw.json", configured: false, currentProvider: null },
  ],
  get_relay_balance: { remaining: 12.34, used: 7.66, low: false, error: null },
  get_autostart_status: true,
  get_product_info: {
    name: "Aimyria 一键配置工具",
    version: "1.0.0",
    website: "https://aimyria.com",
    docs: "https://aimyria.com/docs",
    support: "https://aimyria.com/support",
    license: "MIT",
    attribution: "Based on CC Switch (MIT) by Jason Young",
  },
  configure_all: { configured: ["claude", "codex", "gemini", "opencode", "openclaw"], failed: [], warnings: [] },

  get_mcp_servers: [
    {
      id: "context7",
      name: "context7",
      server: { command: "npx", args: ["-y", "@upstash/context7-mcp"] },
      apps: { claude: true, codex: false, gemini: false, opencode: true, openclaw: false },
      description: "查库文档",
    },
    {
      id: "playwright",
      name: "Playwright",
      server: { command: "npx", args: ["-y", "@playwright/mcp@latest"] },
      apps: { claude: false, codex: true, gemini: false, opencode: false, openclaw: false },
    },
    {
      id: "filesystem",
      name: "filesystem",
      server: { command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem", "D:/work"] },
      apps: { claude: true, codex: true, gemini: true, opencode: false, openclaw: true },
    },
  ],

  get_installed_skills: [
    {
      id: "pdf-tools",
      name: "pdf-tools",
      directory: "skills/pdf-tools",
      description: "读 PDF、填表、合并",
      apps: {
        claude: true, "claude-desktop": false, codex: false, gemini: false,
        grokbuild: false, opencode: true, openclaw: false, hermes: false,
        pi: false, mcode: false,
      },
      installedAt: now - 86400 * 6,
      updatedAt: now - 3600,
    },
    {
      id: "deploy",
      name: "deploy-helper",
      directory: "skills/deploy-helper",
      description: "生成发布命令并核对配置",
      apps: {
        claude: false, "claude-desktop": false, codex: true, gemini: false,
        grokbuild: false, opencode: false, openclaw: false, hermes: false,
        pi: false, mcode: false,
      },
      installedAt: now - 86400 * 2,
      updatedAt: now - 86400 * 2,
    },
  ],
  check_skill_updates: { updates: [], failures: [] },
  scan_unmanaged_skills: [
    {
      directory: "legacy-notes",
      name: "legacy-notes",
      description: "旧版笔记助手",
      foundIn: ["claude"],
      path: "C:/Users/a/.claude/skills/legacy-notes",
    },
  ],
  resync_skills_to_apps: [
    { app: "claude", ok: true, failedSkills: [] },
    { app: "codex", ok: true, failedSkills: [] },
  ],

  get_prompts: {
    concise: {
      id: "concise",
      name: "简洁回答",
      content: "回答控制在三句话以内。先给结论，再给理由。",
      enabled: true,
    },
    review: {
      id: "review",
      name: "代码审查",
      content: "先说安全性，再说正确性，最后说风格。每条都给出文件和行号。",
      enabled: false,
    },
  },
  get_prompt_file_location: { path: "C:/Users/a/.claude/CLAUDE.md", displayPath: "~/.claude/CLAUDE.md" },

  get_usage_summary: { today: { requests: 0, tokens: 0, cost: 0 }, total: { requests: 0, tokens: 0, cost: 0 } },
};

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".woff2": "font/woff2",
};

/** The shim, injected ahead of the app's module script (modules are deferred). */
function shimScript() {
  return `
<script>
(function () {
  var FIXTURES = ${JSON.stringify(FIXTURES)};
  var calls = [];
  window.__CAPTURED_CALLS__ = calls;
  window.__TAURI_INTERNALS__ = {
    invoke: function (cmd, args) {
      calls.push({ cmd: cmd, args: args });
      if (Object.prototype.hasOwnProperty.call(FIXTURES, cmd)) return Promise.resolve(FIXTURES[cmd]);
      // Anything unlisted resolves to null rather than rejecting: a screen that
      // probes an optional command should show its empty state, not an error
      // banner about a command this preview simply does not model.
      return Promise.resolve(null);
    },
    transformCallback: function (cb) { return 1; },
    convertFileSrc: function (p) { return p; },
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main" } },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    registerListener: function () { return Promise.resolve(function () {}); },
    unregisterListener: function () { return Promise.resolve(); },
  };
})();
</script>
`;
}

/**
 * Opens a tab by name once the app has mounted.
 *
 * Reaching a tab normally means clicking, which needs a visible focused window —
 * and this desktop has other apps constantly taking the foreground, which is
 * exactly what made live-window screenshots unreliable. Selecting the tab from
 * the URL removes the click, so the screenshot depends only on this server.
 *
 * Harness-side only: it dispatches on the tab buttons the app already renders,
 * so what gets screenshotted is the real component.
 */
function openTabScript(tab) {
  if (!tab) return "";
  return `
<script>
(function () {
  var want = ${JSON.stringify(tab)};
  var tries = 0;
  (function attempt() {
    var tabs = document.querySelectorAll('[role="tab"]');
    for (var i = 0; i < tabs.length; i++) {
      if (tabs[i].textContent.trim() === want) { tabs[i].click(); return; }
    }
    // Retry across frames rather than guessing a mount delay.
    if (++tries < 90) requestAnimationFrame(attempt);
  })();
})();
</script>
`;
}

const server = createServer((req, res) => {
  const parsed = new URL(req.url, "http://localhost");
  const tab = parsed.searchParams.get("tab");

  if (parsed.pathname === "/__preview.html") {
    const indexPath = join(DIST, "index.html");
    if (!existsSync(indexPath)) {
      res.writeHead(500).end("index.html missing");
      return;
    }
    let html = readFileSync(indexPath, "utf8");
    // Insert before the first <script> so it wins the race with the module.
    html = html.replace(/<script/i, shimScript() + openTabScript(tab) + "<script");
    res.writeHead(200, { "content-type": MIME[".html"] }).end(html);
    return;
  }

  const url = parsed.pathname === "/" ? "/index.html" : parsed.pathname;
  const file = join(DIST, url);
  if (!file.startsWith(DIST) || !existsSync(file)) {
    res.writeHead(404).end("not found: " + url);
    return;
  }
  res.writeHead(200, { "content-type": MIME[extname(file)] || "application/octet-stream" });
  res.end(readFileSync(file));
});

server.listen(PORT, "127.0.0.1", () => {
  const assets = existsSync(join(DIST, "assets"))
    ? readdirSync(join(DIST, "assets")).join(", ")
    : "(no assets dir)";
  console.log(`preview on http://127.0.0.1:${PORT}/__preview.html`);
  console.log(`  assets: ${assets}`);
});
