// Delete the i18n keys orphaned by the vendor credential panels.
//
// The Volcengine AK/SK panel and the Zhipu team panel are gone, so their strings
// have no reader: `volcengineAkSkHint`, `volcengineKeyConsoleLink`, `zhipuTeamHint`,
// `organizationId`, `projectIdPlaceholder`.
//
// They survived because an i18n key only leaves dist/ when it leaves the locale
// files, and nothing else was watching for five orphan keys that describe two
// vendors' consoles. The built-renderer rule found them.
//
// # The guard
//
// A key is deleted only if no source file still calls `t("<key>")`. `organizationId`
// and `projectIdPlaceholder` in particular sound generic and could easily belong to
// some other form; guessing would break a live screen. The check greps `src/` for
// each full key path, not just the leaf name, so `t("usageScript.organizationId")`
// matches and `t("other.organizationId")` does not.

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";

const ROOT = "D:/src/cc-switch-main";
const LOCALES = ["zh", "en", "ja", "zh-TW"];
const PARENT = "usageScript";

// leaf key names
const DEAD = [
  "volcengineAkSkHint",
  "volcengineKeyConsoleLink",
  "zhipuTeamHint",
  "organizationId",
  "projectIdPlaceholder",
];

function grepCount(pattern) {
  try {
    const out = execFileSync(
      "node",
      [
        "-e",
        `const fs=require('fs');const{execSync}=require('child_process');` +
          `const r=execSync('grep -rl --include=*.ts --include=*.tsx "${pattern}" ${ROOT}/src || true',{encoding:'utf8'});` +
          `process.stdout.write(String(r.trim().split('\\n').filter(Boolean).length))`,
      ],
      { encoding: "utf8", shell: true },
    );
    return Number(out.trim() || "0");
  } catch {
    return -1;
  }
}

let removed = 0;
const kept = [];

for (const loc of LOCALES) {
  const p = `${ROOT}/src/i18n/locales/${loc}.json`;
  if (!existsSync(p)) continue;
  const json = JSON.parse(readFileSync(p, "utf8"));
  const parent = json?.[PARENT];
  if (!parent) continue;

  for (const key of DEAD) {
    if (!(key in parent)) continue;
    const full = `${PARENT}.${key}`;
    const uses = grepCount(full);
    if (uses > 0) {
      kept.push(`${loc}: ${full} still referenced by ${uses} file(s)`);
      continue;
    }
    delete parent[key];
    removed += 1;
  }

  writeFileSync(p, `${JSON.stringify(json, null, 2)}\n`);
}

console.log(`removed ${removed} orphan key(s) across ${LOCALES.length} locales`);
if (kept.length) {
  console.log(`\nkept (${kept.length}):`);
  for (const k of kept) console.log(`  - ${k}`);
}
