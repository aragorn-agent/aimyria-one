//! Live-config import for the additive-mode clients.
//!
//! Upstream's `services/provider/live.rs`, lines 1290-1460, extracted verbatim.
//! The rest of that file cannot be copied: it opens by importing
//! `proxy::providers::codex_oauth_auth` and carries a `LiveSyncOutcome::ProxyMode`
//! branch for provider takeover, neither of which means anything in a build that
//! points clients straight at the relay.
//!
//! These two functions are the exception — they read a client's own config file
//! and upsert rows, with no proxy in the path. Keeping them verbatim means the
//! import behaviour matches upstream exactly.

use crate::error::AppError;
use crate::hermes_config;
use crate::openclaw_config::{self, OpenClawProviderConfig};
use crate::provider::Provider;
use crate::store::AppState;

pub fn import_openclaw_providers_from_live(state: &AppState) -> Result<usize, AppError> {
    use crate::openclaw_config;

    let providers = openclaw_config::get_typed_providers()?;
    if providers.is_empty() {
        return Ok(0);
    }

    let mut imported = 0;
    let mut updated = 0;
    let existing_ids = state.db.get_provider_ids("openclaw")?;

    for (id, config) in providers {
        // Validate: skip entries with empty id or no models
        if id.trim().is_empty() {
            log::warn!("Skipping OpenClaw provider with empty id");
            continue;
        }
        if config.models.is_empty() {
            log::warn!("Skipping OpenClaw provider '{id}': no models defined");
            continue;
        }

        // Convert to Value for settings_config
        let settings_config = match serde_json::to_value(&config) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("Failed to serialize OpenClaw provider '{id}': {e}");
                continue;
            }
        };

        if existing_ids.contains(&id) {
            match state.db.get_provider_by_id(&id, "openclaw") {
                Ok(Some(existing)) => {
                    if existing.settings_config != settings_config {
                        let mut provider = existing;
                        provider.settings_config = settings_config;
                        if let Err(e) = state.db.save_provider("openclaw", &provider) {
                            log::warn!(
                                "Failed to update OpenClaw provider '{id}' from live config: {e}"
                            );
                        } else {
                            updated += 1;
                            log::info!("Updated OpenClaw provider '{id}' from live config");
                        }
                    }
                }
                Ok(None) => {
                    log::warn!("OpenClaw provider '{id}' disappeared while importing live config")
                }
                Err(e) => log::warn!("Failed to look up OpenClaw provider '{id}': {e}"),
            }
            continue;
        }

        // Determine display name: use first model name if available, otherwise use id
        let display_name = config
            .models
            .first()
            .and_then(|m| m.name.clone())
            .unwrap_or_else(|| id.clone());

        // Create provider
        let mut provider = Provider::with_id(id.clone(), display_name, settings_config, None);
        provider.meta = Some(crate::provider::ProviderMeta {
            live_config_managed: Some(true),
            ..Default::default()
        });

        // Save to database
        if let Err(e) = state.db.save_provider("openclaw", &provider) {
            log::warn!("Failed to import OpenClaw provider '{id}': {e}");
            continue;
        }

        imported += 1;
        log::info!("Imported OpenClaw provider '{id}' from live config");
    }

    Ok(imported + updated)
}

/// Import all providers from Hermes live config to database
///
/// This imports existing providers from ~/.hermes/config.yaml
/// into the CC Switch database. Each provider found will be added to the
/// database with is_current set to false.
pub fn import_hermes_providers_from_live(state: &AppState) -> Result<usize, AppError> {
    use crate::hermes_config;

    let providers = hermes_config::get_providers()?;
    if providers.is_empty() {
        return Ok(0);
    }

    let mut imported = 0;
    let mut updated = 0;
    let existing_ids = state.db.get_provider_ids("hermes")?;

    for (name, config) in providers {
        // Validate: skip entries with empty name
        if name.trim().is_empty() {
            log::warn!("Skipping Hermes provider with empty name");
            continue;
        }

        if existing_ids.contains(&name) {
            match state.db.get_provider_by_id(&name, "hermes") {
                Ok(Some(existing)) => {
                    if existing.settings_config != config {
                        let mut provider = existing;
                        provider.settings_config = config;
                        if let Err(e) = state.db.save_provider("hermes", &provider) {
                            log::warn!(
                                "Failed to update Hermes provider '{name}' from live config: {e}"
                            );
                        } else {
                            updated += 1;
                            log::info!("Updated Hermes provider '{name}' from live config");
                        }
                    }
                }
                Ok(None) => {
                    log::warn!("Hermes provider '{name}' disappeared while importing live config")
                }
                Err(e) => log::warn!("Failed to look up Hermes provider '{name}': {e}"),
            }
            continue;
        }

        // Create provider
        let mut provider = Provider::with_id(name.clone(), name.clone(), config, None);
        provider.meta = Some(crate::provider::ProviderMeta {
            live_config_managed: Some(true),
            ..Default::default()
        });

        // Save to database
        if let Err(e) = state.db.save_provider("hermes", &provider) {
            log::warn!("Failed to import Hermes provider '{name}': {e}");
            continue;
        }

        imported += 1;
        log::info!("Imported Hermes provider '{name}' from live config");
    }

    Ok(imported + updated)
}

/// Remove a Hermes provider from live config
///
/// This removes a specific provider from ~/.hermes/config.yaml
/// without affecting other providers in the file.
pub fn remove_hermes_provider_from_live(provider_id: &str) -> Result<(), AppError> {
    use crate::hermes_config;

    // Check if Hermes config directory exists
    if !hermes_config::get_hermes_dir().exists() {
        log::debug!("Hermes config directory doesn't exist, skipping removal of '{provider_id}'");
        return Ok(());
    }

    hermes_config::remove_provider(provider_id)?;
    log::info!("Hermes provider '{provider_id}' removed from live config");

    Ok(())
}

/// Remove an OpenClaw provider from live config
///
/// This removes a specific provider from ~/.openclaw/openclaw.json
/// without affecting other providers in the file.
pub fn remove_openclaw_provider_from_live(provider_id: &str) -> Result<(), AppError> {
    use crate::openclaw_config;

    // Check if OpenClaw config directory exists
    if !openclaw_config::get_openclaw_dir().exists() {
        log::debug!("OpenClaw config directory doesn't exist, skipping removal of '{provider_id}'");
        return Ok(());
    }

    openclaw_config::remove_provider(provider_id)?;
    log::info!("OpenClaw provider '{provider_id}' removed from live config");

    Ok(())
}

