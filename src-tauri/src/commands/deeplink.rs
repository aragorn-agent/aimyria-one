use crate::deeplink::{
    import_mcp_from_deeplink, import_prompt_from_deeplink, import_provider_from_deeplink,
    import_skill_from_deeplink, parse_deeplink_url, DeepLinkImportRequest,
};
use crate::store::AppState;
use tauri::State;

/// Claim the deep link that arrived before the UI could receive it.
///
/// `handle_deeplink_url` emits `deeplink-import`, but on a cold start that emit
/// happens during `setup()` — before the webview loads and before the dialog's
/// listener exists — and Tauri drops events nobody is listening for. Without this
/// the window opens with no confirmation dialog and the import silently never
/// happens; see `crate::deeplink::stash_pending_deeplink`.
///
/// Destructive by design: the dialog calls this once on mount, and again when a
/// live event does arrive, so a request is shown once and never replayed on a
/// later launch.
#[tauri::command]
pub fn take_pending_deeplink() -> Option<DeepLinkImportRequest> {
    let pending = crate::deeplink::take_pending_deeplink();
    if pending.is_some() {
        log::info!("Claimed a deep link stashed before the UI was listening");
    }
    pending
}

/// Parse a deep link URL and return the parsed request for frontend confirmation
#[tauri::command]
pub fn parse_deeplink(url: String) -> Result<DeepLinkImportRequest, String> {
    log::info!("Parsing deep link URL: {}", crate::url_for_log(&url));
    parse_deeplink_url(&url).map_err(|e| e.to_string())
}

/// Merge configuration from Base64/URL into a deep link request
/// This is used by the frontend to show the complete configuration in the confirmation dialog
#[tauri::command]
pub fn merge_deeplink_config(
    request: DeepLinkImportRequest,
) -> Result<DeepLinkImportRequest, String> {
    log::info!("Merging config for deep link request: {:?}", request.name);
    crate::deeplink::parse_and_merge_config(&request).map_err(|e| e.to_string())
}

/// Import a provider from a deep link request (legacy, kept for compatibility)
#[tauri::command]
pub async fn import_from_deeplink(
    state: State<'_, AppState>,
    request: DeepLinkImportRequest,
) -> Result<String, String> {
    log::info!(
        "Importing provider from deep link: {:?} for app {:?}",
        request.name,
        request.app
    );

    let provider_id = import_provider_from_deeplink(&state, request)
        .await
        .map_err(|e| e.to_string())?;

    log::info!("Successfully imported provider with ID: {provider_id}");

    Ok(provider_id)
}

/// Import resource from a deep link request (unified handler)
#[tauri::command]
pub async fn import_from_deeplink_unified(
    state: State<'_, AppState>,
    request: DeepLinkImportRequest,
) -> Result<serde_json::Value, String> {
    log::info!("Importing {} resource from deep link", request.resource);

    match request.resource.as_str() {
        "provider" => {
            let request = crate::deeplink::provider::merge_group_models(&request)
                .await
                .map_err(|e| e.to_string())?;
            if let Some(app_handle) = request
                .app
                .as_deref()
                .and_then(crate::aimyria_apps::AimyriaOnlyApp::from_str)
            {
                let settings = crate::deeplink::provider::build_aimyria_only_settings(
                    app_handle.as_str(),
                    &request,
                )
                .map_err(|e| e.to_string())?;
                match app_handle {
                    crate::aimyria_apps::AimyriaOnlyApp::WorkBuddy => {
                        crate::aimyria_apps::write_workbuddy(&settings)
                            .map_err(|e| e.to_string())?;
                    }
                    crate::aimyria_apps::AimyriaOnlyApp::DeepSeekHarness => {
                        crate::aimyria_apps::write_dsh(&settings).map_err(|e| e.to_string())?;
                    }
                }
                return Ok(serde_json::json!({
                    "type": "provider",
                    "id": app_handle.as_str(),
                    "path": app_handle.live_path().display().to_string()
                }));
            }
            let provider_id = import_provider_from_deeplink(&state, request)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "provider",
                "id": provider_id
            }))
        }
        "prompt" => {
            let prompt_id =
                import_prompt_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "prompt",
                "id": prompt_id
            }))
        }
        "mcp" => {
            let result = import_mcp_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            // Add type field to the result
            Ok(serde_json::json!({
                "type": "mcp",
                "importedCount": result.imported_count,
                "importedIds": result.imported_ids,
                "failed": result.failed
            }))
        }
        "skill" => {
            let skill_key =
                import_skill_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "skill",
                "key": skill_key
            }))
        }
        _ => Err(format!("Unsupported resource type: {}", request.resource)),
    }
}
