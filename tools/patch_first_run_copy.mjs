// Replace the upstream multi-provider welcome copy with the one-click product
// copy. Literal value replacement, not a JSON round-trip: the locale files are
// large and hand-formatted, and reserialising them would rewrite every line for
// no reason.
//
// Idempotent: running it twice finds the new values already in place and
// leaves the file alone.

import { readFileSync, writeFileSync } from "node:fs";

const COPY = {
  zh: {
    title: "欢迎使用 Aimyria 一键配置工具",
    bodyDefault:
      "打开 aimyria.com/keys，在你正在用的密钥右侧点「导入到 CCS」，选择要接入的应用并确认，工具才会写入配置。",
    bodyOfficial:
      "只写你选择的应用；原有的钩子、权限和模型偏好都会保留。密钥只保存在本机，工具常驻托盘，需要时再打开即可。",
    confirm: "开始接入",
  },
  "zh-TW": {
    title: "歡迎使用 Aimyria 一鍵設定工具",
    bodyDefault:
      "開啟 aimyria.com/keys，在你正在使用的金鑰右側點「匯入到 CCS」，選擇要接上的應用程式並確認，工具才會寫入設定。",
    bodyOfficial:
      "只會寫入你選擇的應用程式；原有的掛鉤、權限與模型偏好都會保留。金鑰只保存在本機，工具常駐系統匣，需要時再打開即可。",
    confirm: "開始設定",
  },
  en: {
    title: "Welcome to Aimyria One",
    bodyDefault:
      "Open aimyria.com/keys and choose “Import to CCS” next to the key you want to use. Pick the tools to connect and confirm — only then are their config files written.",
    bodyOfficial:
      "Only the apps you pick are touched. Existing hooks, permissions and model preferences stay exactly as they are. Your key never leaves this machine, and the tool lives in the tray.",
    confirm: "Get started",
  },
  ja: {
    title: "Aimyria One へようこそ",
    bodyDefault:
      "aimyria.com/keys を開き、使うキーの「CCS にインポート」を選びます。接続するアプリを選んで確認してから、初めて設定ファイルが書き込まれます。",
    bodyOfficial:
      "書き込むのは選んだアプリだけです。既存のフック・権限・モデル設定はそのまま残ります。キーはこの PC にのみ保存され、ツールはタスクトレイに常駐します。",
    confirm: "開始する",
  },
};

for (const [lang, copy] of Object.entries(COPY)) {
  const path = `src/i18n/locales/${lang}.json`;
  let text = readFileSync(path, "utf8");
  const current = JSON.parse(text).firstRunNotice;
  let changed = 0;
  for (const key of ["title", "bodyDefault", "bodyOfficial", "confirm"]) {
    const before = JSON.stringify(current[key]);
    const after = JSON.stringify(copy[key]);
    if (before === after) continue;
    if (!text.includes(before)) {
      throw new Error(`${lang}: ${key} not found verbatim in ${path}`);
    }
    text = text.replace(before, after);
    changed += 1;
  }
  if (changed > 0) writeFileSync(path, text);
  console.log(`${lang}: ${changed} value(s) updated`);
}
