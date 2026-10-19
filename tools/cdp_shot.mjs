// Screenshot / inspect the running Tauri app over the WebView2 CDP endpoint.
//
// The desktop window cannot be captured reliably on this machine (other windows
// steal the foreground), so the WebView itself is the source of truth: launch the
// app with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 and
// talk to it here.
//
// Usage:
//   node tools/cdp_shot.mjs out.png [port] [expression]
//
// The optional expression is evaluated in the page and printed, which is how the
// smoke test reads the visible screen text without a screenshot.

import { writeFileSync } from "node:fs";

const [out = "shot.png", portArg = "9222", expr] = process.argv.slice(2);
const port = Number(portArg);

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

await send("Page.enable");
await send("Runtime.enable");
await new Promise((r) => setTimeout(r, 400));

if (expr) {
  const res = await send("Runtime.evaluate", {
    expression: expr,
    awaitPromise: true,
    returnByValue: true,
  });
  console.log(JSON.stringify(res.result?.value ?? res, null, 2));
}

const shot = await send("Page.captureScreenshot", { format: "png" });
writeFileSync(out, Buffer.from(shot.data, "base64"));
console.log(`wrote ${out}`);
ws.close();
