// Ask the running app what it rendered, through the DevTools protocol.
//
// This exists because "the binary contains the new bundle" is a claim about bytes.
// What matters is what the user sees, and those differ when the running process is
// stale, when a cached asset is served, or when the frontend throws before mount.
//
// Usage: node tools/inspect_ui.mjs [port]

const port = Number(process.argv[2] ?? 9333);

const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === 'page');
if (!page) {
  console.error('no page target');
  process.exit(1);
}
console.log(`page: ${page.title} ${page.url}`);

const ws = new WebSocket(page.webSocketDebuggerUrl);

let nextId = 0;
const pending = new Map();

function evaluate(expression) {
  const id = ++nextId;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    ws.send(
      JSON.stringify({
        id,
        method: 'Runtime.evaluate',
        params: { expression, returnByValue: true, awaitPromise: true },
      }),
    );
    setTimeout(() => {
      if (pending.has(id)) {
        pending.delete(id);
        reject(new Error('timeout'));
      }
    }, 8000);
  });
}

ws.addEventListener('message', (event) => {
  const data = JSON.parse(event.data);
  const entry = pending.get(data.id);
  if (!entry) return;
  pending.delete(data.id);
  if (data.error) return entry.reject(new Error(JSON.stringify(data.error)));
  entry.resolve(data.result?.result?.value);
});

await new Promise((resolve, reject) => {
  ws.addEventListener('open', resolve, { once: true });
  ws.addEventListener('error', reject, { once: true });
});

const navLabels = await evaluate(
  `Array.from(document.querySelectorAll('nav a, nav button, [role=navigation] *')).map(e => e.textContent.trim()).filter(t => t && t.length < 12).join('|')`,
);

const bodyText = await evaluate(`document.body.innerText.replace(/\\s+/g, ' ').slice(0, 300)`);

const hasRelayKeys = await evaluate(`document.body.innerText.includes('密钥')`);
const hasOverview = await evaluate(`document.body.innerText.includes('概览')`);

console.log(`nav     : ${navLabels}`);
console.log(`body    : ${bodyText}`);
console.log('');
console.log(`contains 密钥 (keys page)  : ${hasRelayKeys}`);
console.log(`contains 概览 (overview)   : ${hasOverview}`);

ws.close();
process.exit(hasRelayKeys && hasOverview ? 0 : 1);
