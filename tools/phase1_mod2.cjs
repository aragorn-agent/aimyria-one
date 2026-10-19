// M3 phase 1 · services/provider/mod.rs cleanup.
//
// Everything removed here belonged to features that no longer exist: Pi,
// universal providers, the `live::` module, and Claude model-key
// normalisation (which existed for the deleted editor projection).
'use strict';
const fs = require('fs');
const p = 'D:\\src\\cc-switch-main\\src-tauri\\src\\services\\provider\\mod.rs';
let L = fs.readFileSync(p, 'utf8').split(/\r?\n/);

const at = (txt, from = 0) => { for (let i = from; i < L.length; i++) if (L[i].includes(txt)) return i; return -1; };
const rm = (a, b) => L.splice(a, b - a + 1);
function replace(a, b, lines) { L.splice(a, b - a + 1, ...lines.split('\n')); }

// ── 1. drop my duplicate free `extract_claude_common_config` ───────────────
{
  const s = at('/// The Claude Code settings that belong to a provider rather than the user.');
  const e = at('/// Re-write every supported app', s);
  if (s < 0 || e < 0) { console.error('1: anchors'); process.exit(1); }
  rm(s, e);
}

// ── 2. `normalize_provider_if_claude` no longer normalises anything ────────
{
  const s = at('fn normalize_provider_if_claude(app_type: &AppType, provider: &mut Provider) {');
  const e = L.findIndex((l, k) => k > s && l === '    }');
  replace(s, e, [
    '    /// Claude model keys are normalised by the relay, not by us: the row we',
    '    /// store is exactly what the editor showed. Kept as a seam so a future',
    '    /// pass has one place to hook into.',
    '    fn normalize_provider_if_claude(_app_type: &AppType, _provider: &mut Provider) {}',
  ].join('\n'));
}

// ── 3. drop `check_live_config_exists` (was the `live::` bridge) ──────────
{
  const s = at('    /// Check whether a provider exists in live config');
  const e = L.findIndex((l, k) => k > s && l === '    }');
  rm(s, e + 1);
}

// ── 4. drop the Pi branch in `list` ───────────────────────────────────────
{
  const s = at('if app_type == AppType::Pi {');
  if (s >= 0) rm(s, s + 2);
}

// ── 5. implement the three live-config entry points ───────────────────────
{
  const s = at('    /// Returns `Ok(true)` if imported, `Ok(false)` if skipped.');
  const e = at('    /// Get custom endpoints list (re-export)', s);
  if (s < 0 || e < 0) { console.error('5: anchors'); process.exit(1); }
  replace(s, e, `    /// Import the client's own current config as a provider row.
    ///
    /// Upstream did this on first launch so a user who had already configured
    /// their CLI by hand did not have to start from an empty list. We keep it:
    /// it is how an existing user adopts the tool without losing their setup.
    ///
    /// Returns \`Ok(true)\` when a row was created, \`Ok(false)\` when there was
    /// nothing to import or a row already exists.
    pub fn import_default_config(state: &AppState, app_type: AppType) -> Result<bool, AppError> {
        let live = Self::read_live_settings(app_type.clone())?;
        if live.as_object().is_none_or(|o| o.is_empty()) {
            return Ok(false);
        }
        if !state
            .db
            .get_all_providers(app_type.as_str())?
            .values()
            .any(|p| Self::is_builtin_row(p))
        {
            return Ok(false);
        }

        let provider = Provider::with_id(
            "imported".to_string(),
            format!("{} (imported)", app_type.as_str()),
            live,
            None,
        );
        Self::validate_provider_settings(&app_type, &provider)?;
        state.db.save_provider(app_type.as_str(), &provider)?;
        log::info!("✓ Imported existing {} config as a provider row", app_type.as_str());
        Ok(true)
    }

    /// Whether startup should try \`import_default_config\` for this app.
    ///
    /// Only once per app, and never after the user has any row of their own.
    pub fn should_import_default_config_on_startup(
        state: &AppState,
        app_type: &AppType,
    ) -> Result<bool, AppError> {
        if state
            .db
            .get_all_providers(app_type.as_str())?
            .values()
            .any(|p| !Self::is_builtin_row(p))
        {
            return Ok(false);
        }
        Ok(state
            .db
            .get_setting(&format!("imported_default_{}", app_type.as_str()))?
            .is_none())
    }

    /// True for the placeholder row \`import_default_config\` writes.
    fn is_builtin_row(provider: &Provider) -> bool {
        provider.id == "imported"
    }

    /// Read the client's current live config.
    ///
    /// A missing file is not an error: it means the client has never been
    /// configured, which is a normal state on a fresh machine.
    pub fn read_live_settings(app_type: AppType) -> Result<Value, AppError> {
        let Some(path) = crate::live_writer::live_path(&app_type) else {
            return Ok(Value::Object(serde_json::Map::new()));
        };
        if !path.exists() {
            return Ok(Value::Object(serde_json::Map::new()));
        }
        match std::fs::read_to_string(&path) {
            Ok(text) if app_type == AppType::Codex => Ok(serde_json::json!({
                "auth": crate::read_json_file::<Value>(&crate::codex_config::get_codex_auth_path())
                    .unwrap_or_else(|_| Value::Object(serde_json::Map::new())),
                "config": text,
            })),
            Ok(text) => serde_json::from_str(&text).map_err(|e| {
                AppError::InvalidInput(format!(
                    "{} 配置不是合法 JSON: {e}",
                    app_type.as_str()
                ))
            }),
            Err(_) => Ok(Value::Object(serde_json::Map::new())),
        }
    }

`);
}

// ── 6. drop the usage-script validation block ─────────────────────────────
{
  const s = at('        // Validate and clean UsageScript configuration');
  const e = L.findIndex((l, k) => k > s && l.trim() === '}');
  rm(s, e - s);
}

// ── 7. drop the universal-provider impl block ─────────────────────────────
{
  const s = at('use crate::provider::UniversalProvider;');
  const e = at('pub struct ProviderSortUpdate {', s);
  if (s >= 0 && e > s) {
    const start = s - 2;                 // the `// ====` banner above the use
    const end = L.length - 1;            // through EOF (it is the last item)
    let last = end;
    while (last > e && L[last].trim() === '') last--;
    rm(start, last + 1);
  }
}

fs.writeFileSync(p, L.join('\n'), 'utf8');
console.log(`mod.rs -> ${L.length} lines`);