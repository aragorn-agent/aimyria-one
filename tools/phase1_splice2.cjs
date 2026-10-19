// M3 · Phase 1 (cont.): replace the last mode/MCP/OMO-dependent functions.
'use strict';
const fs = require('fs');
const file = 'D:\\src\\cc-switch-main\\src-tauri\\src\\services\\provider\\mod.rs';
const L = fs.readFileSync(file, 'utf8').split('\n');

const repl = [
  // Orphan MCP sync left over from `switch` (MCP was removed in M2).
  { from: 649, to: 659, text: `        result
    }` },

  // `remove_from_live_config`: OMO / additive-app branches are gone.
  { from: 518, to: 567, text: `    /// Drop a provider row's footprint from the client's live config.
    ///
    /// Phase 1 supports Claude Code and Codex, which own a single file each.
    /// For them this is a no-op: the file holds whatever provider is current,
    /// and removing a non-current row must not touch it.
    pub fn remove_from_live_config(
        state: &AppState,
        app_type: AppType,
        id: &str,
    ) -> Result<(), AppError> {
        let current_id = Self::current(state, app_type.clone())?;
        if current_id == id {
            // The row being removed is the applied one; the client keeps its
            // current config until another provider is switched in.
            log::info!(
                "移除的是当前生效的 {} 供应商 {id}，live 配置保持不变",
                app_type.as_str()
            );
        }
        Ok(())
    }` },

  // `delete`: OMO queue handling is gone.
  { from: 462, to: 511, text: `    /// Remove a provider row.
    ///
    /// The row's live footprint is handled by \`remove_from_live_config\`; for
    /// the apps supported in phase 1 that is a no-op, so this only needs to
    /// clear the "current" pointer when it pointed at the row being deleted.
    pub fn delete(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        if Self::current(state, app_type.clone())? == id {
            crate::settings::set_current_provider(&app_type, None)?;
        }
        state.db.delete_provider(app_type.as_str(), id)?;
        Ok(())
    }` },

  // `extract_credentials`: Claude Desktop support was removed in M2.
  { from: 1390, to: 1393, text: `            AppType::ClaudeDesktop => Err(AppError::Message(
                "Claude Desktop is not supported by this build".to_string(),
            )),` },
];

for (const r of repl) {
  if (r.from < 1 || r.to > L.length || r.from > r.to) {
    console.error(`BAD RANGE ${r.from}-${r.to} (file has ${L.length} lines)`);
    process.exit(1);
  }
}
repl.sort((a, b) => b.from - a.from);
for (const r of repl) {
  const head = (L[r.from - 1] || '').trim().slice(0, 60);
  console.log(`  L${r.from}-L${r.to}  REPLACE  "${head}"`);
  L.splice(r.from - 1, r.to - r.from + 1, ...(r.text ? r.text.split('\n') : []));
}
fs.writeFileSync(file, L.join('\n'), 'utf8');
console.log(`\nnow ${L.length} lines`);