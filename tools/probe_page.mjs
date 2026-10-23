// Open a page in the running app and report what it renders.
// Usage: node tools/probe_page.mjs <port> <navLabel>

const port = Number(process.argv[2] ?? 9336);
const navLabel = process.argv[3] ?? '概览';

const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === 'page');
if (!page) {
  console.error('no page target');
  process.exit(1);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
let nextId = 0;
const pending = new Map();

function send(method, params = {}) {
  const id = ++nextId;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
    setTimeout(() => {
      if (pending.has(id)) {
        pending.delete(id);
        reject(new Error(`timeout: ${method}`));
      }
    }, 10000);
  });
}

function evaluate(expression) {
  return send('Runtime.evaluate', {
    expression,
    returnByValue: true,
    awaitPromise: true,
  }).then((r) => r?.result?.value);
}

ws.addEventListener('message', (event) => {
  const data = JSON.parse(event.data);
  const entry = pending.get(data.id);
  if (!entry) return;
  pending.delete(data.id);
  if (data.error) return entry.reject(new Error(JSON.stringify(data.error)));
  entry.resolve(data.result);
});

await new Promise((resolve, reject) => {
  ws.addEventListener('open', resolve, { once: true });
  ws.addEventListener('error', reject, { once: true });
});

// Click the nav entry, the same way a user would.
const clicked = await evaluate(`
  (() => {
    const target = '${navLabel}';
    const all = Array.from(document.querySelectorAll('button, a, [role=button]'));
    const hit = all.find(e => e.textContent.trim() === target);
    if (!hit) return 'not found';
    hit.click();
    return 'clicked';
  })()
`);
console.log(`nav click (${navLabel}): ${clicked}`);

await new Promise((r) => setTimeout(r, 1500));

const text = await evaluate(`document.body.innerText.replace(/\\s+/g, ' ').slice(0, 900)`);
console.log('--- rendered text ---');
console.log(text);

ws.close();
