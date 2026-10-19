// Corrective pass: the previous repoint mapped
//   D:\中转站运营\_aimyria-one  ->  D:\src\cc-switch-main
// but the recorded targets included the trailing `\cc-switch-main`, so every
// link now points one level too deep:
//   D:\src\cc-switch-main\cc-switch-main\node_modules\...
//
// This collapses that duplicate segment back to the real location.
'use strict';
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const BAD = 'D:\\src\\cc-switch-main\\cc-switch-main';
const GOOD = 'D:\\src\\cc-switch-main';
const ROOT = path.join(GOOD, 'node_modules');

function findLinks(dir, out = []) {
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const e of entries) {
    const p = path.join(dir, e.name);
    if (e.isSymbolicLink()) {
      out.push(p);
      continue;
    }
    let st;
    try {
      st = fs.lstatSync(p);
    } catch {
      continue;
    }
    if (st.isDirectory()) findLinks(p, out);
  }
  return out;
}

const links = findLinks(ROOT);
console.log('scanning ' + links.length + ' links');

let fixed = 0;
const failures = [];

for (const link of links) {
  let target;
  try {
    target = fs.readlinkSync(link);
  } catch {
    continue;
  }
  if (!target.toLowerCase().startsWith(BAD.toLowerCase())) continue;

  const rewritten = GOOD + target.slice(BAD.length);

  try {
    fs.unlinkSync(link);
  } catch {
    try {
      execFileSync('cmd', ['/c', 'rmdir', link], { stdio: 'ignore' });
    } catch {
      failures.push(link + ': cannot remove');
      continue;
    }
  }

  try {
    execFileSync('cmd', ['/c', 'mklink', '/J', link, rewritten], { stdio: 'ignore' });
    fixed++;
  } catch (e) {
    failures.push(link + ': mklink failed (' + e.status + ')');
  }
}

console.log('corrected: ' + fixed);
if (failures.length) {
  console.log('failures: ' + failures.length);
  for (const f of failures.slice(0, 10)) console.log('  ' + f);
}

// Verify the three the build actually needs.
for (const name of ['typescript', 'vite', 'react', 'react-dom']) {
  const p = path.join(ROOT, name);
  let ok = false;
  try {
    ok = fs.existsSync(path.join(p, 'package.json'));
  } catch {
    ok = false;
  }
  console.log((ok ? '  OK      ' : '  BROKEN  ') + name);
}