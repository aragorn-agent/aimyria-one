const fs = require('fs');
const R = 'D:\\src\\cc-switch-main\\src-tauri\\src\\';

// commands/provider.rs: trailing orphan `#[tauri::command]` from the
// universal-provider command block we removed.
{
  const p = R + 'commands\\provider.rs';
  let L = fs.readFileSync(p, 'utf8').split(/\r?\n/);
  const out = [];
  for (let i = 0; i < L.length; i++) {
    // drop a trailing attribute that no longer has an item after it
    if (L[i].trim() === '#[tauri::command]' && i >= L.length - 4) continue;
    out.push(L[i]);
  }
  while (out.length && out[out.length - 1].trim() === '') out.pop();
  out.push('');
  fs.writeFileSync(p, out.join('\n'), 'utf8');
  console.log('  commands/provider.rs -> ' + out.length);
}

// stream_check.rs: `resolve_base_url` lost its closing brace.
{
  const p = R + 'services\\stream_check.rs';
  let L = fs.readFileSync(p, 'utf8').split(/\r?\n/);
  const s = L.findIndex(l => l.includes('    /// 将探测原始结果包装成'));
  if (s < 0) { console.error('anchor not found'); process.exit(1); }
  L.splice(s, 0, '    }', '');
  fs.writeFileSync(p, L.join('\n'), 'utf8');
  console.log('  stream_check.rs -> ' + L.length);
}