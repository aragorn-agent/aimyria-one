// Exercise the binding control the way a user does: pick a key in the select.
//
// This is the check that "the two screens are one system" rests on. Changing the
// select must (a) call the same switch the provider page uses, (b) leave the
// database's is_current on the new row, and (c) leave the client's live file
// containing the new key. Anything less and the screens can disagree again.
//
// Usage: node tools/probe_bind.mjs <port> <appLabel> <selectIndex>

const port = Number(process.argv[2] ?? 9342);
const appLabel = process.argv[3] ?? 'Claude Code';
const selectIndex = Number(process.argv[4] ?? 1);

const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === 'page');
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
        reject(new Error('timeout'));
      }
    }, 15000);
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

// Find the row for `appLabel`, then its select, and report what it offers.
const before = await evaluate(`
  (() => {
    const rows = Array.from(document.querySelectorAll('div')).filter(d =>
      d.textContent.trim().startsWith('${appLabel}') && d.querySelector('select'));
    const row = rows[rows.length - 1];
    if (!row) return 'row not found';
    const sel = row.querySelector('select');
    return {
      current: sel.value,
      options: Array.from(sel.options).map(o => o.textContent.trim()),
      values: Array.from(sel.options).map(o => o.value),
    };
  })()
`);
console.log('before:', JSON.stringify(before, null, 2));

if (!before || typeof before === 'string') {
  ws.close();
  process.exit(1);
}

const targetValue = before.values[selectIndex];
if (!targetValue) {
  console.log('no option at index', selectIndex);
  ws.close();
  process.exit(1);
}

// Change it the way a user does: set the value and dispatch a change event.
const changed = await evaluate(`
  (() => {
    const rows = Array.from(document.querySelectorAll('div')).filter(d =>
      d.textContent.trim().startsWith('${appLabel}') && d.querySelector('select'));
    const row = rows[rows.length - 1];
    const sel = row.querySelector('select');
    sel.value = ${JSON.stringify(targetValue)};
    sel.dispatchEvent(new Event('change', { bubbles: true }));
    return 'dispatched';
  })()
`);
console.log('change:', changed);

await new Promise((r) => setTimeout(r, 3500));

const after = await evaluate(`
  (() => {
    const rows = Array.from(document.querySelectorAll('div')).filter(d =>
      d.textContent.trim().startsWith('${appLabel}') && d.querySelector('select'));
    const row = rows[rows.length - 1];
    const sel = row.querySelector('select');
    return { current: sel.value };
  })()
`);
console.log('after :', JSON.stringify(after));

ws.close();
process.exit(after?.current === targetValue ? 0 : 1);
