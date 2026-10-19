// M3 · Phase 1: replace the tray's tier/subscription quota rendering.
// Upstream grouped coding-plan and official-subscription tiers into quota
// lines. Both features are gone; the only usage source left is our own
// /v1/usage balance endpoint, which returns a single entry.
'use strict';
const fs = require('fs');
const file = 'D:\\src\\cc-switch-main\\src-tauri\\src\\tray.rs';
let L = fs.readFileSync(file, 'utf8').split(/\r?\n/);

const repl = [
  // highest → lowest
  { from: 725, to: 756, text: `/// Quota for the provider currently in use.
///
/// Upstream also consulted managed-OAuth and app-subscription caches here.
/// Those backends were removed in M2, so the script cache (fed by our own
/// /v1/usage balance query) is the only source left.
fn usage_view(
    usage_cache: &UsageCache,
    texts: &TrayTexts,
    app_type: &AppType,
    provider: &Provider,
    provider_id: &str,
) -> Option<QuotaView> {
    // A disabled or deleted script must not keep showing a stale snapshot.
    if tray_usage_source(app_type, provider).is_none() {
        usage_cache.invalidate_script(app_type, provider_id);
        return None;
    }
    // Another provider's balance must never be shown under this one.
    usage_cache.invalidate_subscription(app_type);
    usage_cache
        .with_script(app_type, provider_id, |result| {
            format_script_result(texts, result)
        })
        .flatten()
}` },

  { from: 620, to: 666, text: `/// Render a usage result as tray lines.
///
/// Our balance endpoint returns a single entry, so each \`UsageData\` becomes
/// one line. Upstream additionally reassembled named tiers (coding-plan and
/// official-subscription quotas); that grouping is gone with those features.
fn format_script_result(
    texts: &TrayTexts,
    result: &crate::provider::UsageResult,
) -> Option<QuotaView> {
    if !result.success {
        return Some(QuotaView::Failed(QuotaFailure::Other));
    }
    let data = result.data.as_ref()?;

    let lines: Vec<QuotaLine> = data
        .iter()
        .map(|entry| {
            // Prefer the remaining amount; fall back to what was used so a
            // response without \`remaining\` still shows a number.
            let amount = match (entry.remaining, entry.used) {
                (Some(remaining), _) => remaining,
                (None, Some(used)) => used,
                _ => 0.0,
            };
            let unit = entry.unit.clone().unwrap_or_default();
            let rendered = if unit.is_empty() {
                format!("{amount}")
            } else {
                format!("{amount} {unit}")
            };
            QuotaLine {
                text: fill(texts.balance, &[("value", rendered.as_str())]),
                left: entry.remaining.unwrap_or_default(),
                label: None,
                resets_at: resets_at_from_extra(entry.extra.as_deref()),
            }
        })
        .collect();

    (!lines.is_empty()).then_some(QuotaView::Lines(lines))
}` },

  { from: 552, to: 553, text: `` },

  { from: 547, to: 550, text: `/// Group known tiers into quota lines.
///
/// Kept as a single pass so \`TierEntry\` stays meaningful: with no tiered
/// plans in this build there is nothing to group, so entries render as-is.
fn grouped_tier_lines(texts: &TrayTexts, entries: &[TierEntry<'_>]) -> Vec<QuotaLine> {
    let _ = texts;
    entries
        .iter()
        .map(|entry| QuotaLine {
            text: entry.name.to_string(),
            left: (100.0 - entry.utilization).round().max(0.0),
            label: None,
            resets_at: entry.resets_at.clone(),
        })
        .collect()
}` },
];

for (const r of repl) {
  if (r.from < 1 || r.to > L.length || r.from > r.to) {
    console.error(`BAD RANGE ${r.from}-${r.to} (file has ${L.length})`);
    process.exit(1);
  }
}
repl.sort((a, b) => b.from - a.from);
for (const r of repl) {
  console.log(`  L${r.from}-L${r.to}  ${r.text ? 'REPLACE' : 'delete '}  "${(L[r.from - 1] || '').trim().slice(0, 48)}"`);
  L.splice(r.from - 1, r.to - r.from + 1, ...(r.text ? r.text.split('\n') : []));
}
fs.writeFileSync(file, L.join('\n'), 'utf8');
console.log(`\ntray.rs -> ${L.length} lines`);