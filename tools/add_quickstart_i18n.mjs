// Add the `quickstart.*` and `nav.quickstart` / `nav.balance` keys to all locales.
//
// Written rather than hand-edited: the four locale files are 18,966 lines between
// them, and this block has to land in all four identically or the i18n coverage
// test fails with a key that only exists in Chinese.
//
// Every string carries an English `defaultValue` as well, so a locale that has not
// been translated yet shows readable text instead of the raw key. That is how the
// rest of this file is written; see `notifications.openLinkFailed` in App.tsx.
//
// Idempotent: a second run reports "already present" and changes nothing.

import { readFileSync, writeFileSync } from "node:fs";

const DIR = "D:/src/cc-switch-main/src/i18n/locales";

/** Per-locale strings. Keys are identical across locales; only the values differ. */
const STRINGS = {
  zh: {
    "nav.quickstart": "一键配置",
    "nav.balance": "${{amount}}",
    "quickstart.title": "一键配置",
    "quickstart.configureAll": "一键配置全部",
    "quickstart.reconfigureAll": "重新写入全部",
    "quickstart.balanceTitle": "中转站余额",
    "quickstart.balanceLow": "余额不足",
    "quickstart.balanceLowHint": "低于 $2 时客户端会用不了。充值后无需重装，直接重试即可。",
    "quickstart.manageAccount": "管理账号",
    "quickstart.clientsTitle": "客户端",
    "quickstart.progress": "${{done}} / ${{total}} 已配置",
    "quickstart.loading": "正在读取…",
    "quickstart.noPath": "该客户端不支持写入",
    "quickstart.configure": "配置",
    "quickstart.reconfigure": "重新配置",
    "quickstart.reportDone": "已配置 ${{count}} 个客户端",
    "quickstart.reportPartial": "已配置 ${{done}} / ${{total}} 个，其余见下方说明",
  },
  "zh-TW": {
    "nav.quickstart": "一鍵配置",
    "nav.balance": "${{amount}}",
    "quickstart.title": "一鍵配置",
    "quickstart.configureAll": "一鍵設定全部",
    "quickstart.reconfigureAll": "重新寫入全部",
    "quickstart.balanceTitle": "中轉站餘額",
    "quickstart.balanceLow": "餘額不足",
    "quickstart.balanceLowHint": "低於 $2 時客戶端會用不了。儲值後不需重裝，直接重試即可。",
    "quickstart.manageAccount": "管理帳號",
    "quickstart.clientsTitle": "客戶端",
    "quickstart.progress": "${{done}} / ${{total}} 已設定",
    "quickstart.loading": "正在讀取…",
    "quickstart.noPath": "此客戶端不支援寫入",
    "quickstart.configure": "設定",
    "quickstart.reconfigure": "重新設定",
    "quickstart.reportDone": "已設定 ${{count}} 個客戶端",
    "quickstart.reportPartial": "已設定 ${{done}} / ${{total}} 個，其餘見下方說明",
  },
  en: {
    "nav.quickstart": "Quick setup",
    "nav.balance": "${{amount}}",
    "quickstart.title": "Quick setup",
    "quickstart.configureAll": "Configure all",
    "quickstart.reconfigureAll": "Rewrite all",
    "quickstart.balanceTitle": "Relay balance",
    "quickstart.balanceLow": "Balance running low",
    "quickstart.balanceLowHint":
      "Clients stop working below $2. Top up and retry — no reinstall needed.",
    "quickstart.manageAccount": "Manage account",
    "quickstart.clientsTitle": "Clients",
    "quickstart.progress": "${{done}} / ${{total}} configured",
    "quickstart.loading": "Loading…",
    "quickstart.noPath": "This client cannot be written",
    "quickstart.configure": "Configure",
    "quickstart.reconfigure": "Reconfigure",
    "quickstart.reportDone": "Configured ${{count}} clients",
    "quickstart.reportPartial":
      "Configured ${{done}} of ${{total}}; see the notes below",
  },
  ja: {
    "nav.quickstart": "クイック設定",
    "nav.balance": "${{amount}}",
    "quickstart.title": "クイック設定",
    "quickstart.configureAll": "すべて設定",
    "quickstart.reconfigureAll": "すべて再書き込み",
    "quickstart.balanceTitle": "リレーの残高",
    "quickstart.balanceLow": "残高が不足しています",
    "quickstart.balanceLowHint":
      "$2 を下回るとクライアントが使えなくなります。チャージ後、再試行のみで再インストールは不要です。",
    "quickstart.manageAccount": "アカウント管理",
    "quickstart.clientsTitle": "クライアント",
    "quickstart.progress": "${{done}} / ${{total}} 設定済み",
    "quickstart.loading": "読み込み中…",
    "quickstart.noPath": "このクライアントには書き込めません",
    "quickstart.configure": "設定",
    "quickstart.reconfigure": "再設定",
    "quickstart.reportDone": "${{count}} 個のクライアントを設定しました",
    "quickstart.reportPartial":
      "${{total}} 個中 ${{done}} 個を設定しました。詳細は下をご覧ください",
  },
};

/**
 * English fallbacks, used both as the `defaultValue` at runtime and as the value
 * written into any locale that has no explicit translation.
 */
const EN = STRINGS.en;

function deepSet(obj, dottedKey, value) {
  const parts = dottedKey.split(".");
  let node = obj;
  for (let i = 0; i < parts.length - 1; i++) {
    if (typeof node[parts[i]] !== "object" || node[parts[i]] === null) {
      node[parts[i]] = {};
    }
    node = node[parts[i]];
  }
  const last = parts[parts.length - 1];
  if (Object.prototype.hasOwnProperty.call(node, last)) return false;
  node[last] = value;
  return true;
}

let added = 0;
let skipped = 0;

for (const [locale, strings] of Object.entries(STRINGS)) {
  const path = `${DIR}/${locale}.json`;
  let json;
  try {
    json = JSON.parse(readFileSync(path, "utf8"));
  } catch (e) {
    console.error(`  ${locale}: cannot read — ${e.message}`);
    process.exit(1);
  }
  let n = 0;
  for (const [key, value] of Object.entries(strings)) {
    if (deepSet(json, key, value)) n += 1;
    else {
      skipped += 1;
      console.error(`  ${locale}: ${key} already exists — refusing to overwrite`);
      process.exit(1);
    }
  }
  // Trailing newline matches how the file is written upstream.
  writeFileSync(path, `${JSON.stringify(json, null, 2)}\n`);
  added += n;
  console.log(`  ${locale}: +${n} keys`);
}

console.log("");
console.log(`added ${added} keys, skipped ${skipped}`);
