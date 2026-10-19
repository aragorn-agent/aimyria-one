// Delete `providerForm.partnerPromotion` from all four locales.
//
// # What it is
//
// Forty strings per locale — the banner copy that appears under the API-key field
// when a preset names a `partnerPromotionKey`. Some carried discount codes:
// `"PPIO is a leading independent Agentic Cloud provider… register via this link as
// a new user for 10% off your first top-up!"`, and SubRouter's "…one key access
// multiple models at ~30% of direct API cost".
//
// The new built-renderer brand rule caught two of them in the bundle, which is how
// they were found. The other thirty-eight ship the same way; the rule reports one
// problem per brand per file, not one per string.
//
// # Why deleting the copy is safe
//
// Every `partnerPromotionKey` in the codebase is read off a preset:
// `activePreset?.partnerPromotionKey`, `preset.partnerPromotionKey`,
// `entry.preset.partnerPromotionKey`, and one `setPartnerPromotionKey(preset.…)`.
// The preset arrays were emptied for the 1A decision, so no preset carries a key,
// so nothing ever sets one, so the banner never renders and the lookup never runs.
//
// The call sites stay. They are how this would work if a preset ever came back, and
// removing fifty-odd prop threads across every form is a different job with a
// different risk profile. Dead but harmless beats a refactor nobody asked for.
//
// What is not harmless is shipping a competitor's advertisement in our binary.

import { readFileSync, writeFileSync, existsSync } from "node:fs";

const LOCALES = ["zh", "en", "ja", "zh-TW"];
const SECTION = "partnerPromotion";

let total = 0;
for (const loc of LOCALES) {
  const p = `D:/src/cc-switch-main/src/i18n/locales/${loc}.json`;
  if (!existsSync(p)) continue;
  const json = JSON.parse(readFileSync(p, "utf8"));
  const node = json?.providerForm?.[SECTION];
  if (!node) {
    console.log(`  ${loc}: already absent`);
    continue;
  }
  const n = Object.keys(node).length;
  delete json.providerForm[SECTION];
  writeFileSync(p, `${JSON.stringify(json, null, 2)}\n`);
  total += n;
  console.log(`  ${loc}: removed ${n} partnerPromotion strings`);
}

console.log("");
console.log(`total ${total} strings across ${LOCALES.length} locales`);
