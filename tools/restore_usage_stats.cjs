// usage_stats.rs: batch12's `splice(s, s+6, ...)` deleted six lines counted
// from the `ModelPricing::from_strings(` call site, which sat inside a closure
// — so it ate that closure's tail plus the head of the next function. Restore
// the pre-batch12 file and re-add only the ModelPricing constructor that the
// surviving call site actually needs.
'use strict';
const fs = require('fs');

const LIVE = 'D:\\\\src\\\\cc-switch-main\\\\src-tauri\\\\src\\\\services\\\\usage_stats.rs';
const SNAP = 'D:\\\\backup\\\\aimyria-one-M6\\\\src-tauri\\\\src\\\\services\\\\usage_stats.rs';

let L = fs.readFileSync(SNAP, 'utf8').split(/\r?\n/);
console.log('restored ' + L.length + ' lines from M6');

if (L.findIndex(l => l.includes('impl ModelPricing')) < 0) {
  // Insert right after the struct's closing brace.
  const i = L.findIndex(l => l.startsWith('pub struct ModelPricing {'));
  if (i < 0) { console.error('ModelPricing struct not found in snapshot'); process.exit(1); }
  let e = -1;
  for (let k = i; k < L.length; k++) if (L[k] === '}') { e = k; break; }

  L.splice(e + 1, 0, ...[
    '',
    'impl ModelPricing {',
    '    /// Build a price row from the strings the pricing tables store.',
    '    ///',
    '    /// Unparseable numbers become 0 rather than an error: a bad price row',
    '    /// should cost one model its price, not the whole price table.',
    '    pub fn from_strings(',
    '        model_id: &str,',
    '        input: &str,',
    '        output: &str,',
    '        cache_read: &str,',
    '        cache_creation: &str,',
    '    ) -> Self {',
    '        let num = |v: &str| v.trim().parse::<f64>().unwrap_or(0.0);',
    '        Self {',
    '            model_id: model_id.to_string(),',
    '            input_price: num(input),',
    '            output_price: num(output),',
    '            cache_read_price: num(cache_read),',
    '            cache_creation_price: num(cache_creation),',
    '        }',
    '    }',
    '}',
  ]);
}

fs.writeFileSync(LIVE, L.join('\n'), 'utf8');
console.log('wrote ' + L.length + ' lines');