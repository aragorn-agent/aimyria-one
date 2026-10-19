// Click through every app and global page, then open "Add provider" on each
// app, reporting which step lands on the error boundary.
//
// One Runtime.evaluate per step: a reload inside an evaluate destroys the
// context and aborts the whole sweep, so reloads happen here, between calls.
//
// Usage: node tools/cdp_sweep.mjs [port]

const port = Number(process.argv[2] || 9222);
const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === "page");
if (!page) throw new Error("no page target");

const ws = new WebSocket(page.webSocketDebuggerUrl);
let nextId = 1;
const pending = new Map();
ws.addEventListener("message", (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
  }
});
await new Promise((resolve, reject) => {
  ws.addEventListener("open", resolve, { once: true });
  ws.addEventListener("error", reject, { once: true });
});
function send(method, params = {}) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });
}
await send("Runtime.enable");
await send("Page.enable");

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const evaluate = async (expression) => {
  const res = await send("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
    userGesture: true,
  });
  return res.result?.value;
};

const clickExpr = (label, wait = 1100) => `(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const nodes = [...document.querySelectorAll('button,a,[role="button"],[role="menuitem"]')];
  const labelOf = (n) => (n.getAttribute("aria-label") || n.innerText || "").trim();
  const hit = nodes.find((n) => labelOf(n) === ${JSON.stringify(label)}) ||
    nodes.find((n) => labelOf(n).includes(${JSON.stringify(label)}));
  if (!hit) return { clicked: false, err: document.body.innerText.includes("界面遇到了问题") };
  hit.click();
  await sleep(${wait});
  return { clicked: true, err: document.body.innerText.includes("界面遇到了问题") };
})()`;

const reload = async () => {
  await send("Page.reload", { ignoreCache: false });
  await sleep(2600);
};

const out = [];
const apps = ["Claude Code", "Claude Desktop", "Codex", "Gemini CLI", "Grok Build", "OpenCode", "OpenClaw", "Hermes", "Pi", "MiniMax Code"];
const globals = ["接入中心", "用量统计", "应用", "设置"];

for (const label of [...apps, ...globals]) {
  const r = await evaluate(clickExpr(label));
  out.push({ step: "page:" + label, ...r });
  if (r?.err) {
    await reload();
    continue;
  }
  if (apps.includes(label)) {
    const add = await evaluate(clickExpr("添加供应商", 900));
    out.push({ step: "add:" + label, ...add });
    if (add?.err) {
      await reload();
      continue;
    }
    await evaluate(`(() => {
      const nodes = [...document.querySelectorAll('button,a,[role="button"]')];
      const labelOf = (n) => (n.getAttribute("aria-label") || n.innerText || "").trim();
      const close = nodes.find((n) => ["取消","关闭","返回"].includes(labelOf(n)));
      if (close) close.click();
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      return true;
    })()`);
    await sleep(500);
    if (await evaluate(`document.body.innerText.includes("自定义配置 · 手动填写所有字段")`)) {
      await reload();
    }
  }
}

console.log(JSON.stringify(out, null, 2));
ws.close();
