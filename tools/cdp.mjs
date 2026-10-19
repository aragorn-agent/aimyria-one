// Minimal CDP driver: evaluate JS, click by text, reload, screenshot, read console.
//
// Kept separate from cdp_shot.mjs because these commands need to sequence
// (click -> wait -> screenshot) and survive a page reload, which a single
// Runtime.evaluate cannot.
//
// Usage:
//   node tools/cdp.mjs eval "<expression>"
//   node tools/cdp.mjs click "<visible text>"
//   node tools/cdp.mjs reload [ms]
//   node tools/cdp.mjs shot <file.png>
//   node tools/cdp.mjs text
//   node tools/cdp.mjs console

import { writeFileSync } from "node:fs";

const [cmd, arg] = process.argv.slice(2);
const port = Number(process.env.CDP_PORT || 9222);
const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === "page");
if (!page) throw new Error("no page target");

const ws = new WebSocket(page.webSocketDebuggerUrl);
let nextId = 1;
const pending = new Map();
const logs = [];
ws.addEventListener("message", (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.method === "Runtime.consoleAPICalled") {
    logs.push({
      type: msg.params.type,
      text: msg.params.args.map((a) => a.value ?? a.description ?? "").join(" "),
    });
  }
  if (msg.method === "Runtime.exceptionThrown") {
    logs.push({
      type: "exception",
      text: msg.params.exceptionDetails.exception?.description ?? msg.params.exceptionDetails.text,
    });
  }
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

switch (cmd) {
  case "eval": {
    const res = await send("Runtime.evaluate", {
      expression: arg,
      awaitPromise: true,
      returnByValue: true,
      userGesture: true,
    });
    console.log(JSON.stringify(res.result?.value ?? res, null, 2));
    break;
  }
  case "click": {
    const expr = `(() => {
      const wanted = ${JSON.stringify(arg)};
      const nodes = [...document.querySelectorAll('button,[role="button"],a,[role="menuitem"],[role="tab"]')];
      const label = (n) => (n.innerText || n.getAttribute('aria-label') || '').trim();
      const hit = nodes.find((n) => label(n) === wanted)
        || nodes.find((n) => label(n).includes(wanted));
      if (!hit) return { clicked: false, candidates: nodes.map(label).filter(Boolean).slice(0, 60) };
      hit.click();
      return { clicked: true, tag: hit.tagName, text: (hit.innerText || '').trim() };
    })()`;
    const res = await send("Runtime.evaluate", { expression: expr, returnByValue: true, userGesture: true });
    console.log(JSON.stringify(res.result?.value, null, 2));
    break;
  }
  case "reload": {
    await send("Page.reload", { ignoreCache: false });
    await sleep(Number(arg || 3500));
    console.log("reloaded");
    break;
  }
  case "shot": {
    const shot = await send("Page.captureScreenshot", { format: "png" });
    writeFileSync(arg || "shot.png", Buffer.from(shot.data, "base64"));
    console.log(`wrote ${arg || "shot.png"}`);
    break;
  }
  case "text": {
    const res = await send("Runtime.evaluate", {
      expression: "document.body.innerText",
      returnByValue: true,
    });
    console.log(res.result?.value ?? "");
    break;
  }
  case "console": {
    console.log(JSON.stringify(logs, null, 2));
    break;
  }
  default:
    throw new Error(`unknown command ${cmd}`);
}
ws.close();
