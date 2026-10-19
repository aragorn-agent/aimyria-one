// speedtest.rs: `build_client`'s body was lost in the M2 cuts and my previous
// pass wrote only its signature line (rep() takes a string, not a spread array).
'use strict';
const fs = require('fs');
const p = 'D:\\\\src\\\\cc-switch-main\\\\src-tauri\\\\src\\\\services\\\\speedtest.rs';

let L = fs.readFileSync(p, 'utf8').split(/\r?\n/);

const s = L.findIndex(l => l.includes('fn build_client(timeout_secs: u64)'));
if (s < 0) { console.error('anchor not found'); process.exit(1); }
// The signature is the last line of the file with a stray `}` after it.
if (L[s + 1] !== '}') { console.error('unexpected shape at L' + (s + 2) + ': ' + JSON.stringify(L[s + 1])); process.exit(1); }

L.splice(s, 2, ...[
  '    fn build_client(timeout_secs: u64) -> Result<(Client, std::time::Duration), AppError> {',
  '        let timeout = std::time::Duration::from_secs(timeout_secs);',
  '        let client = Client::builder()',
  '            .timeout(timeout)',
  '            .build()',
  '            .map_err(|e| AppError::Message(format!("failed to build HTTP client: {e}")))?;',
  '        Ok((client, timeout))',
  '    }',
  '',
]);

fs.writeFileSync(p, L.join('\n'), 'utf8');
console.log('speedtest.rs -> ' + L.length + ' lines');