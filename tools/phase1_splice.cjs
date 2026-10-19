// M3 · Phase 1: rewire services/provider/mod.rs onto the new live_writer.
// Applies a set of line-range replacements (1-based, inclusive) in one pass,
// highest range first so earlier ranges stay valid.
'use strict';
const fs = require('fs');
const file = 'D:\\src\\cc-switch-main\\src-tauri\\src\\services\\provider\\mod.rs';
const L = fs.readFileSync(file, 'utf8').split('\n');

const cut = (a, b) => ({ from: a, to: b, text: null });

const repl = [
  // ── dead / mode-only helpers ───────────────────────────────────────────
  cut(1674, 1857),                    // scrub_leaked_gemini_common_config (Gemini-only, used live::)
  cut(105, 113),                       // enum EditorSaveKind + impl (mode-only)
  cut(72, 88),                         // reapply_current_codex_official_live (codex_direct)
  cut(990, 1023),                      // update_codex (codex_direct)
  cut(767, 804),                       // update_claude_from_editor (mode)
  cut(737, 764),                       // resync_proxy_for_saved_row (proxy)
  cut(711, 733),                       // keep_row_if_written (mode)
  cut(644, 707),                       // save_gemini_or_grok_from_editor (mode)
  cut(583, 639),                       // save_codex_from_editor (mode)
  cut(443, 515),                       // editor_view / editor_view_with_meta / editor_category (phase 2)
  cut(406, 439),                       // add_codex (codex_direct)

  // ── live_writer-backed replacements ────────────────────────────────────
  { from: 1298, to: 1350, text: `    /// Upstream migrated a legacy "common config" snippet out of every
    /// provider row as part of its multi-provider Stack feature. Aimyria One
    /// has a single provider and never creates a snippet, so there is nothing
    /// to migrate.
    pub fn migrate_legacy_common_config_usage(
        _state: &AppState,
        _app_type: AppType,
        _legacy_snippet: &str,
    ) -> Result<(), AppError> {
        Ok(())
    }` },

  { from: 1283, to: 1296, text: `    /// Re-write the current provider's config into the client.
    ///
    /// Called on startup and after settings changes so a config the user edited
    /// by hand gets pulled back in line with the stored provider.
    pub fn sync_current_provider_for_app(
        state: &AppState,
        app_type: AppType,
    ) -> Result<(), AppError> {
        let current_id = Self::current(state, app_type.clone())?;
        if current_id.is_empty() {
            return Ok(());
        }
        Self::switch(state, app_type, &current_id).map(|_| ())
    }` },

  { from: 1216, to: 1261, text: `    /// Switch implementation for every app, additive or not.
    fn switch_normal(
        state: &AppState,
        app_type: AppType,
        id: &str,
        providers: &indexmap::IndexMap<String, Provider>,
    ) -> Result<SwitchResult, AppError> {
        let provider = providers
            .get(id)
            .ok_or_else(|| AppError::Message(format!("供应商 {id} 不存在")))?;

        if app_type.is_additive_mode() {
            // Additive apps (OpenCode / OpenClaw) keep every provider in one
            // file, so a "switch" only records which one the tray reports.
            log::info!("[SWITCH] {} 选中 {id}（累加式应用）", app_type.as_str());
            return Ok(SwitchResult::default());
        }

        // Refuse before touching the client's file: a row whose key is still a
        // placeholder would otherwise overwrite a working config.
        crate::live_writer::ensure_ready(&app_type, &provider.settings_config)?;
        crate::live_writer::write(&app_type, &provider.settings_config)?;

        log::info!(
            "[SWITCH] {} 写入 {id} -> {}",
            app_type.as_str(),
            crate::live_writer::live_path(&app_type)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<未支持>".to_string()),
        );
        Ok(SwitchResult::default())
    }` },

  { from: 1175, to: 1213, text: `    fn switch_inner(
        state: &AppState,
        app_type: AppType,
        id: &str,
    ) -> Result<SwitchResult, AppError> {
        let providers = state.db.get_all_providers(app_type.as_str())?;
        let result = Self::switch_normal(state, app_type, id, &providers);

        if result.is_ok() {
            crate::settings::set_current_provider(&app_type, Some(id))?;
        }
        result
    }` },

  { from: 806, to: 981, text: `    /// Persist an edited provider row, then re-write live if this row is the
    /// one currently applied.
    pub fn update(
        state: &AppState,
        app_type: AppType,
        original_id: Option<&str>,
        provider: Provider,
    ) -> Result<bool, AppError> {
        let mut provider = provider;
        let original_id = original_id.unwrap_or(provider.id.as_str()).to_string();

        Self::normalize_provider_if_claude(&app_type, &mut provider);
        Self::validate_provider_settings(&app_type, &provider)?;
        Self::normalize_usage_script_credential_overrides(&app_type, &mut provider);
        keep_common_config_for_old_versions(&mut provider);

        let is_current = Self::current(state, app_type.clone())? == provider.id;

        if original_id != provider.id {
            if state
                .db
                .get_provider_by_id(&provider.id, app_type.as_str())?
                .is_some()
            {
                return Err(AppError::Message(format!(
                    "供应商 ID {} 已存在",
                    provider.id
                )));
            }
            state.db.save_provider(app_type.as_str(), &provider)?;
            state.db.delete_provider(app_type.as_str(), &original_id)?;
        } else {
            state.db.save_provider(app_type.as_str(), &provider)?;
        }

        if is_current {
            crate::live_writer::ensure_ready(&app_type, &provider.settings_config)?;
            crate::live_writer::write(&app_type, &provider.settings_config)?;
        }
        Ok(true)
    }` },

  { from: 547, to: 578, text: `    /// Persist a provider edited through the UI.
    ///
    /// Phase 1 of the write-layer rewrite routes every app through \`update\`.
    /// The field-level editor diff is ignored for now: the editor modules
    /// depended on the removed \`mode/\` engine and are rebuilt in phase 2.
    pub fn update_from_editor(
        state: &AppState,
        app_type: AppType,
        original_id: Option<&str>,
        provider: Provider,
        _editor: Option<EditorSave>,
    ) -> Result<bool, AppError> {
        Self::update(state, app_type, original_id, provider)
    }` },

  { from: 518, to: 543, text: `    /// Persist a provider added through the UI.
    ///
    /// Phase 1 of the write-layer rewrite routes every app through \`add\`; see
    /// \`update_from_editor\` for why the editor diff is not applied yet.
    pub fn add_from_editor(
        state: &AppState,
        app_type: AppType,
        provider: Provider,
        add_to_live: bool,
        _editor: Option<EditorSave>,
    ) -> Result<bool, AppError> {
        Self::add(state, app_type, provider, add_to_live)
    }` },

  { from: 306, to: 401, text: `    /// Store a provider row and, when asked, make it the applied one.
    ///
    /// This is the deep-link import path: \`import_from_deeplink\` calls
    /// \`add(.., true)\` and then \`switch\`, which is what "one-click config"
    /// actually means.
    pub fn add(
        state: &AppState,
        app_type: AppType,
        provider: Provider,
        add_to_live: bool,
    ) -> Result<bool, AppError> {
        let mut provider = provider;
        Self::normalize_provider_if_claude(&app_type, &mut provider);
        Self::validate_provider_settings(&app_type, &provider)?;
        Self::normalize_usage_script_credential_overrides(&app_type, &mut provider);
        keep_common_config_for_old_versions(&mut provider);

        if state
            .db
            .get_provider_by_id(&provider.id, app_type.as_str())?
            .is_some()
        {
            return Err(AppError::Message(format!(
                "供应商 {} 已存在",
                provider.id
            )));
        }

        let previous = state
            .db
            .get_all_providers(app_type.as_str())?
            .values()
            .find(|p| p.id != provider.id)
            .cloned();

        state.db.save_provider(app_type.as_str(), &provider)?;

        if let Err(error) = Self::apply_new_row(state, &app_type, &provider, add_to_live, previous) {
            // Writing live failed: undo the row so the database never claims a
            // provider the client is not actually configured for.
            match error {
                WriteFailure::LiveWrite(err) => {
                    log::warn!("写入 live 失败，回滚供应商行: {err}");
                    state.db.delete_provider(app_type.as_str(), &provider.id)?;
                    return Err(err);
                }
                WriteFailure::Db(err) => return Err(err),
            }
        }
        Ok(true)
    }

    /// Decide whether a freshly added row should also become the applied one,
    /// and write it if so.
    ///
    /// A row only takes over live when it is the first provider for the app, or
    /// when the caller explicitly asked for it (the deep-link import path).
    fn apply_new_row(
        state: &AppState,
        app_type: &AppType,
        provider: &Provider,
        add_to_live: bool,
        previous: Option<Provider>,
    ) -> Result<(), WriteFailure> {
        let current_id = Self::current(state, app_type.clone())
            .map_err(WriteFailure::Db)?;
        let should_write = add_to_live || current_id.is_empty() || current_id == previous.as_ref().map(|p| p.id.clone()).unwrap_or_default();

        if !should_write {
            return Ok(());
        }
        crate::live_writer::ensure_ready(app_type, &provider.settings_config)
            .and_then(|_| crate::live_writer::write(app_type, &provider.settings_config))
            .map_err(WriteFailure::LiveWrite)?;
        crate::settings::set_current_provider(app_type, Some(&provider.id))
            .map_err(WriteFailure::Db)?;
        Ok(())
    }` },

  { from: 280, to: 303, text: `    /// Id of the provider currently applied to the client, or \`""\` when
    /// none has been applied yet.
    pub fn current(state: &AppState, app_type: AppType) -> Result<String, AppError> {
        let _ = state;
        // Additive-mode apps hold every provider at once, so they have no
        // single "current" one.
        if app_type.is_additive_mode() {
            return Ok(String::new());
        }
        Ok(crate::settings::get_current_provider(&app_type).unwrap_or_default())
    }` },
];

// Validate the ranges against the file before touching anything.
for (const r of repl) {
  if (r.from < 1 || r.to > L.length || r.from > r.to) {
    console.error(`BAD RANGE ${r.from}-${r.to} (file has ${L.length} lines)`);
    process.exit(1);
  }
}

repl.sort((a, b) => b.from - a.from);
for (const r of repl) {
  const head = (L[r.from - 1] || '').trim().slice(0, 64);
  const tail = (L[r.to - 1] || '').trim().slice(0, 40);
  console.log(`  L${r.from}-L${r.to}  ${r.text ? `REPLACE (${r.text.split('\n').length} ln)` : 'delete '}  "${head}" .. "${tail}"`);
  const next = r.text ? r.text.split('\n') : [];
  L.splice(r.from - 1, r.to - r.from + 1, ...next);
}

fs.writeFileSync(file, L.join('\n'), 'utf8');
console.log(`\n${file}: ${file.length ? '' : ''}done, now ${L.length} lines`);