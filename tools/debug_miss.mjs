// Why did the rule miss a planted "SubRouter" offer that is provably in the bundle?
//
// The gate printed FAIL for one brand and stayed silent about another in the same
// file. A gate that misses a planted probe is worse than no gate: it looks like
// verification. So: reproduce the rule's own logic against the real bytes and print
// what each step does, instead of reasoning about it.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { createRequire } from "node:module";

const DIST = "D:/src/cc-switch-main/dist/assets";
const f = readdirSync(DIST).find((x) => /\.js$/.test(x) && statSync(`${DIST}/${x}`).size > 1_000_000);
const text = readFileSync(`${DIST}/${f}`, "utf8");

// The COMPETITOR list, copied from the rule.
const COMPETITOR = [
  "MiMo", "xiaomimimo.com", "LongCat", "longcat.chat", "MiniMax", "zhipu",
  "bigmodel.cn", "api.z.ai", "Moonshot", "Kimi", "qwen3-coder",
  "dashscope.aliyuncs.com", "doubao-seed", "api.deepseek.com",
  "api.siliconflow.cn", "ppio", "jiekou", "SubRouter", "TheRouter", "MoArk",
  "openrouter.ai", "qianfan.baidubce.com",
];

for (const brand of ["SubRouter", "Kimi"]) {
  console.log(`\n${brand}:`);
  console.log(`  in COMPETITOR   : ${COMPETITOR.includes(brand)}`);
  console.log(`  in bundle       : ${text.includes(brand)}`);
  const at = text.indexOf(brand);
  if (at === -1) {
    console.log("  -> not in bundle, would not be checked");
    continue;
  }
  const around = text.slice(Math.max(0, at - 80), at + brand.length + 60).replace(/\s+/g, " ");
  console.log(`  first occurrence: …${around.trim()}…`);
  const precedes = text.slice(Math.max(0, at - 60), at);
  console.log(`  catalogue regex : ${/"(?:id|name)":"$/.test(precedes)}`);
  console.log(`  label regex     : ${new RegExp(`label:\\s*["']${brand}`).test(text.slice(at - 30, at + brand.length + 30))}`);
}