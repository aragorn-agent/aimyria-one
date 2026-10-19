// Repair the mojibake introduced when a UTF-8 script was round-tripped through
// PowerShell's ANSI codepage: two string literals in provider/mod.rs lost their
// original characters and lost the closing quote with them.
'use strict';
const fs = require('fs');
const R = 'D:\\\\src\\\\cc-switch-main\\\\src-tauri\\\\src\\\\';

const MSG = '该供应商未配置用量查询脚本';

// ── provider/mod.rs ──────────────────────────────────────────────────────
{
  const p = R + 'services\\\\provider\\\\mod.rs';
  let L = fs.readFileSync(p, 'utf8').split(/\r?\n/);
  let fixed = 0;
  for (let i = 0; i < L.length; i++) {
    if (/^\s*Err\(AppError::Message\(".*\.to_string\(\)\)\)\s*$/.test(L[i]) &&
        // only the two usage-script call sites; other Err(Message(...)) lines are
        // intact and must not be touched.
        L.slice(Math.max(0, i - 8), i).some(l => l.includes('fn query_usage(') || l.includes('fn test_usage_script('))) {
      L[i] = '        Err(AppError::Message("' + MSG + '".to_string()))';
      fixed++;
    }
  }
  // the orphaned const between the two functions is now unused
  const c = L.findIndex(l => l.includes('const USAGE_SCRIPT_UNSUPPORTED: &str'));
  if (c >= 0) {
    L.splice(c - 1, 3);
    fixed++;
  }
  fs.writeFileSync(p, L.join('\n'), 'utf8');
  console.log('  provider/mod.rs: fixed ' + fixed + ', ' + L.length + ' lines');
}

// ── usage_stats.rs: batch12 spliced the wrong line range ─────────────────
{
  const p = R + 'services\\\\usage_stats.rs';
  let L = fs.readFileSync(p, 'utf8').split(/\r?\n/);
  const i = L.findIndex(l => l.includes('ModelPricing::from_row(model_id, input, output, cache_read, cache_creation)'));
  if (i >= 0) {
    // The splice left a dangling `}` above the call: the old argument list
    // closed its own block. Show it so the fix is verifiable.
    console.log('  usage_stats.rs: call at L' + (i + 1));
    for (let k = i - 8; k <= i + 3; k++) {
      console.log('   ' + (k + 1) + ': ' + JSON.stringify(L[k]));
    }
  }
}