//! Provider import from deep link
//!
//! Handles importing provider configurations via aimyria:// URLs.

use super::utils::{decode_base64_param, infer_homepage_from_endpoint};
use super::DeepLinkImportRequest;
use crate::aimyria_apps;
use crate::error::AppError;
use crate::provider::{ClaudeDesktopMode, Provider, ProviderMeta, UsageScript};
use crate::services::ProviderService;
use crate::store::AppState;
use crate::AppType;
use serde_json::{json, Value};
use std::str::FromStr;

/// Import a provider from a deep link request
///
/// This function:
/// 1. Validates the request
/// 2. Merges config file if provided (v3.8+)
/// 3. Converts it to a Provider structure
/// 4. Delegates to ProviderService for actual import
/// 5. Optionally sets as current provider if enabled=true
pub async fn import_provider_from_deeplink(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<String, AppError> {
    // Verify this is a provider request
    if request.resource != "provider" {
        return Err(AppError::InvalidInput(format!(
            "Expected provider resource, got '{}'",
            request.resource
        )));
    }

    // Step 1: Merge config file if provided (v3.8+)
    let mut merged_request = parse_and_merge_config(&request)?;

    // Extract required fields (now as Option)
    let app_str = merged_request
        .app
        .clone()
        .ok_or_else(|| AppError::InvalidInput("Missing 'app' field for provider".to_string()))?;

    let api_key = merged_request.api_key.as_ref().ok_or_else(|| {
        AppError::InvalidInput("API key is required (either in URL or config file)".to_string())
    })?;

    if api_key.is_empty() {
        return Err(AppError::InvalidInput(
            "API key cannot be empty".to_string(),
        ));
    }

    // Get endpoint: supports comma-separated multiple URLs (first is primary)
    let endpoint_str = merged_request.endpoint.as_ref().ok_or_else(|| {
        AppError::InvalidInput("Endpoint is required (either in URL or config file)".to_string())
    })?;

    // Parse endpoints: split by comma, first is primary
    let all_endpoints: Vec<String> = endpoint_str
        .split(',')
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .collect();

    let primary_endpoint = all_endpoints
        .first()
        .ok_or_else(|| AppError::InvalidInput("Endpoint cannot be empty".to_string()))?;

    // Auto-infer homepage from endpoint if not provided
    if merged_request
        .homepage
        .as_ref()
        .is_none_or(|s| s.is_empty())
    {
        merged_request.homepage = infer_homepage_from_endpoint(primary_endpoint);
    }

    let homepage = merged_request.homepage.as_ref().ok_or_else(|| {
        AppError::InvalidInput("Homepage is required (either in URL or config file)".to_string())
    })?;

    if homepage.is_empty() {
        return Err(AppError::InvalidInput(
            "Homepage cannot be empty".to_string(),
        ));
    }

    let name = merged_request
        .name
        .clone()
        .ok_or_else(|| AppError::InvalidInput("Missing 'name' field for provider".to_string()))?;

    // Parse app type
    let app_type = AppType::from_str(&app_str)
        .map_err(|_| AppError::InvalidInput(format!("Invalid app type: {app_str}")))?;

    // Build provider configuration based on app type
    let mut provider = build_provider_from_request(&app_type, &merged_request)?;

    // The id depends on whose link this is, and the two cases genuinely differ.
    // See `import_provider_id` for why.
    let endpoint_text = merged_request.endpoint.as_deref().unwrap_or_default();
    let points_at_us = points_at_our_relay(endpoint_text);
    let mut provider_id = import_provider_id(endpoint_text, &name);

    // Our own row is being replaced, so drop the old one instead of accumulating
    // rotations. Only for a link that points at us -- another relay's row is a
    // different provider and must never delete ours.
    if points_at_us
        && state
            .db
            .get_all_providers(app_type.as_str())
            .map(|all| all.contains_key(&provider_id))
            .unwrap_or(false)
    {
        ProviderService::delete(state, app_type.clone(), &provider_id).ok();
    }
    provider.id = provider_id.clone();

    // Use ProviderService to add the provider
    ProviderService::add(state, app_type.clone(), provider, true)?;

    // Add extra endpoints as custom endpoints (skip first one as it's the primary)
    for ep in all_endpoints.iter().skip(1) {
        let normalized = ep.trim().trim_end_matches('/').to_string();
        if !normalized.is_empty() {
            if let Err(e) = ProviderService::add_custom_endpoint(
                state,
                app_type.clone(),
                &provider_id,
                normalized.clone(),
            ) {
                log::warn!(
                    "Failed to add custom endpoint '{}': {e}",
                    crate::url_for_log(&normalized)
                );
            }
        }
    }

    // If enabled=true, set as current provider
    if merged_request.enabled.unwrap_or(false) {
        ProviderService::switch(state, app_type.clone(), &provider_id)?;
        log::info!("Provider '{provider_id}' set as current for {app_type:?}");
    }

    Ok(provider_id)
}

/// Fill `models` and `groupName` from the relay's group whitelist.
///
/// The website normally sends the list. Older links do not, and a hand-made
/// link may carry only one `model`. Reading the same `/ops/group-models`
/// endpoint the website uses makes the desktop import the complete model set
/// in every case. Failure is non-fatal: the caller still has the single model
/// and can write a working configuration.
pub async fn merge_group_models(
    request: &DeepLinkImportRequest,
) -> Result<DeepLinkImportRequest, AppError> {
    let mut merged = request.clone();
    if merged
        .models
        .as_ref()
        .is_some_and(|models| !models.is_empty())
    {
        return Ok(merged);
    }
    if merged.resource != "provider" {
        return Ok(merged);
    }
    let api_key = merged.api_key.clone().unwrap_or_default();
    let endpoint = merged.endpoint.clone().unwrap_or_default();
    if api_key.trim().is_empty() || !endpoint.contains("aimyria.com") {
        return Ok(merged);
    }

    let relay = crate::commands::aimyria::relay_site_base(&endpoint);
    match crate::aimyria_apps::fetch_group_models(&relay, &api_key).await {
        Ok(group) if !group.models.is_empty() => {
            merged.models = Some(group.models);
            if merged.group_name.is_none() {
                merged.group_name = Some(group.group_name);
            }
        }
        Ok(_) => {}
        Err(e) => log::warn!("读取分组模型失败，使用单模型回退: {e}"),
    }
    Ok(merged)
}

/// Whether a link's endpoint points at one of *our* relays.
///
/// This is the whole basis for deciding whether an import replaces an existing
/// row or adds a new one, so it lives on its own rather than inline.
pub(crate) fn points_at_our_relay(endpoint: &str) -> bool {
    endpoint.contains(crate::commands::aimyria::RELAY_HOST)
        || endpoint.contains(crate::commands::aimyria::RELAY_ALT_HOST)
}

/// The provider id a deep-link import should write.
///
/// # The two cases
///
/// A link **to us** is a key rotation: the user has one account with us, and
/// re-importing should replace the row rather than leave a trail of them, so the
/// id is the stable [`crate::live_writer::PROVIDER_ID`]. The caller additionally
/// deletes the previous row first.
///
/// A link **to someone else's relay** is a second provider, not a rotation. The
/// old code wrote our fixed id for those too, so importing another relay silently
/// overwrote our row -- the owner hit exactly this (a FlowNode import replaced
/// Aimyria). A unique id per import is what makes repeated imports accumulate.
/// Upstream minted `{name}-{timestamp}` for the same reason; the timestamp keeps
/// the row recognisable in the list.
///
/// The slug is ASCII-only on purpose: ids travel into file paths and config keys
/// on some clients, where an arbitrary display name is a liability.
pub(crate) fn import_provider_id(endpoint: &str, name: &str) -> String {
    if points_at_our_relay(endpoint) {
        return crate::live_writer::PROVIDER_ID.to_string();
    }

    let slug: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect::<String>()
        .trim_matches(['-', '_'])
        .to_string();
    let slug = if slug.is_empty() {
        "provider".to_string()
    } else {
        slug
    };
    format!("{slug}-{}", chrono::Local::now().format("%Y%m%d%H%M%S"))
}

/// Build a Provider structure from a deep link request
pub(crate) fn build_provider_from_request(
    app_type: &AppType,
    request: &DeepLinkImportRequest,
) -> Result<Provider, AppError> {
    let settings_config = match app_type {
        AppType::Claude | AppType::ClaudeDesktop => build_claude_settings(request),
        AppType::Codex => build_codex_settings(request),
        AppType::Gemini => build_gemini_settings(request),
        AppType::GrokBuild => build_grokbuild_settings(request),
        AppType::OpenCode => build_opencode_settings(request),
        AppType::OpenClaw => build_additive_app_settings(request),
        AppType::Hermes => build_hermes_settings(request),
        AppType::Mcode => {
            return Err(AppError::InvalidInput(
                "Add MCode providers from the MCode page".into(),
            ))
        }
        AppType::Pi => {
            return Err(AppError::InvalidInput(
                "Pi providers must be added from the Pi provider page".to_string(),
            ));
        }
    };

    // Build usage script configuration if provided
    let mut meta = build_provider_meta(request)?;
    if matches!(app_type, AppType::ClaudeDesktop) {
        let meta = meta.get_or_insert_with(ProviderMeta::default);
        // Claude Desktop only accepts `claude-*` model ids in its 3P profile: its
        // validator rejects the whole list ("fail-all") on one foreign name, and
        // a rejected list leaves the model menu empty.
        //
        // So the mode depends on what the group actually sells. A Claude group
        // gets a direct provider with the group's own model ids. A group selling
        // DeepSeek / GLM / etc. cannot be expressed as a Desktop provider at all
        // — those names would be rejected — so it is imported as a **proxy**
        // provider, where Desktop sees synthesised `claude-*` route ids and the
        // local router maps them to the real upstream models.
        meta.claude_desktop_mode = Some(desktop_mode_for_request(request));
        // Both modes need a route map, for different reasons.
        //
        // * Proxy: the route ids are what Desktop sees and the local router
        //   translates to real upstream names.
        // * Direct: the routes are *identity mappings* over the `claude-*` ids the
        //   group actually serves. Without them `inferenceModels` is absent from
        //   the profile, and Desktop opens with an empty model menu — the write
        //   looks successful and the app cannot be used.
        //
        // This was missing for direct mode from the start; the one-click screen
        // could therefore produce a "configured" Claude Desktop with no models.
        meta.claude_desktop_model_routes = if meta.claude_desktop_mode == Some(ClaudeDesktopMode::Proxy) {
            desktop_proxy_routes(request)
        } else {
            desktop_direct_routes(request)
        };
    }

    let endpoint_text = request.endpoint.clone().unwrap_or_default();

    if request.balance_url.is_some() || request.balance_token.is_some() {
        let meta = meta.get_or_insert_with(ProviderMeta::default);
        meta.balance_url = request.balance_url.clone();
        meta.balance_token = request.balance_token.clone();
        // The console may also send a usage script that appends /v1/usage to an
        // endpoint already ending in /v1. That 404s and paints a red "quota not
        // found" on the card. The read-only bridge above is the authority, so
        // disable the script rather than showing a false failure.
        if request.balance_url.is_some() {
            if let Some(script) = meta.usage_script.as_mut() {
                script.enabled = false;
            }
        }
    }

    let icon = request.icon.clone().or_else(|| {
        if endpoint_text.contains("321ai.cc") {
            Some("321ai".to_string())
        } else if endpoint_text.contains("aimyria.com") {
            Some("aimyria".to_string())
        } else {
            None
        }
    });
    // Put our own brand on the row rather than whatever display name the
    // console put in the link: the card should read 321AI / AIMYRIA, not a
    // generic "Aimyria" supplied by an older link.
    let brand_name = if endpoint_text.contains("321ai.cc") {
        Some("321AI".to_string())
    } else if endpoint_text.contains("aimyria.com") {
        Some("AIMYRIA".to_string())
    } else {
        None
    };

    let provider = Provider {
        id: String::new(), // Will be generated by caller
        name: brand_name
            .or_else(|| request.name.clone())
            .unwrap_or_default(),
        settings_config,
        website_url: request.homepage.clone(),
        category: None,
        created_at: None,
        sort_index: None,
        notes: request.notes.clone(),
        meta,
        icon,
        icon_color: None,
        in_failover_queue: false,
    };

    Ok(provider)
}

/// WorkBuddy and dsh are not `AppType`s.
///
/// Keeping them out of the enum avoids dragging two file formats through the
/// provider database, proxy router, tray, session scanner and every upstream
/// page. The one-click path needs exactly a settings payload and a writer, so
/// it goes through this function instead of `build_provider_from_request`.
pub(crate) fn build_aimyria_only_settings(
    app_id: &str,
    request: &DeepLinkImportRequest,
) -> Result<Value, AppError> {
    let app = aimyria_apps::AimyriaOnlyApp::from_str(app_id)
        .ok_or_else(|| AppError::InvalidInput(format!("{app_id} 不是 Aimyria 专属客户端")))?;
    let api_key = request.api_key.as_deref().unwrap_or_default();
    if api_key.trim().is_empty() {
        return Err(AppError::InvalidInput("Missing API key".to_string()));
    }
    let endpoint = get_primary_endpoint(request);
    let models = request.models.clone().unwrap_or_else(|| {
        request
            .model
            .as_ref()
            .filter(|model| !model.trim().is_empty())
            .map(|model| vec![model.clone()])
            .unwrap_or_default()
    });
    let models = order_models_for_app(app_id, &models);

    Ok(match app {
        aimyria_apps::AimyriaOnlyApp::WorkBuddy => {
            aimyria_apps::build_workbuddy_settings(&endpoint, api_key, &models)
        }
        aimyria_apps::AimyriaOnlyApp::DeepSeekHarness => {
            aimyria_apps::build_dsh_settings(&endpoint, api_key, &models)
        }
    })
}

/// Put the newest accepted model first, then keep the rest in relay order.
///
/// dsh and WorkBuddy read the first entry as the default; the relay's order is
/// alphabetical or curated, not "newest". The shared TypeScript picker uses
/// the same rules; the Rust mirror lives here so the import is right even if
/// the website sends the raw group list.
pub(crate) fn order_models_for_app(app_id: &str, models: &[String]) -> Vec<String> {
    let mut ordered = models.to_vec();
    let prefix: Option<&str> = match app_id {
        "claude" | "claude-desktop" => Some("claude-"),
        "codex" => Some("gpt-"),
        "gemini" => Some("gemini-"),
        "workbuddy" | "dsh" | "hermes" | "opencode" | "openclaw" => Some("deepseek-"),
        _ => None,
    };
    if let Some(prefix) = prefix {
        if let Some(index) = ordered.iter().position(|id| id.starts_with(prefix)) {
            let preferred = ordered.remove(index);
            ordered.insert(0, preferred);
        }
    }
    ordered
}

/// Get primary endpoint from request (first one if comma-separated)
fn get_primary_endpoint(request: &DeepLinkImportRequest) -> String {
    request
        .endpoint
        .as_ref()
        .and_then(|ep| ep.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn normalize_deeplink_api_key(api_key: &str) -> String {
    api_key.trim().to_string()
}

fn normalize_deeplink_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn usage_api_key_override(request: &DeepLinkImportRequest) -> Option<String> {
    let usage_api_key = normalize_deeplink_api_key(request.usage_api_key.as_deref()?);
    if usage_api_key.is_empty() {
        return None;
    }

    let provider_api_key = request
        .api_key
        .as_deref()
        .map(normalize_deeplink_api_key)
        .unwrap_or_default();

    if !provider_api_key.is_empty() && usage_api_key == provider_api_key {
        None
    } else {
        Some(usage_api_key)
    }
}

fn usage_base_url_override(request: &DeepLinkImportRequest) -> Option<String> {
    let usage_base_url = normalize_deeplink_base_url(request.usage_base_url.as_deref()?);
    if usage_base_url.is_empty() {
        return None;
    }

    let provider_base_url = normalize_deeplink_base_url(&get_primary_endpoint(request));

    if !provider_base_url.is_empty() && usage_base_url == provider_base_url {
        None
    } else {
        Some(usage_base_url)
    }
}

/// Build provider meta with usage script configuration
fn build_provider_meta(request: &DeepLinkImportRequest) -> Result<Option<ProviderMeta>, AppError> {
    // Check if any usage script fields are provided
    if request.usage_script.is_none()
        && request.usage_enabled.is_none()
        && request.usage_api_key.is_none()
        && request.usage_base_url.is_none()
        && request.usage_access_token.is_none()
        && request.usage_user_id.is_none()
        && request.usage_auto_interval.is_none()
    {
        return Ok(None);
    }

    // Decode usage script code if provided
    let code = if let Some(script_b64) = &request.usage_script {
        let decoded = decode_base64_param("usage_script", script_b64)?;
        String::from_utf8(decoded)
            .map_err(|e| AppError::InvalidInput(format!("Invalid UTF-8 in usage_script: {e}")))?
    } else {
        String::new()
    };

    // Determine enabled state: explicit param only, defaulting to disabled.
    //
    // 「携带了代码」不构成用户的启用决定。此处的输入来自 deeplink——即第三方
    // 构造、经浏览器抵达的不可信载荷——而 `code` 是一段会在查询用量时执行的
    // JavaScript。若以 `!code.is_empty()` 作默认，一条链接就能让脚本在用户
    // 从未勾选过的情况下进入启用态。
    //
    // 要启用，链接必须显式携带 `usageEnabled=true`。注意该参数是**链接作者**的
    // 请求，不构成用户的同意；用户的同意体现在确认框展示了完整脚本正文与启用
    // 徽章之后仍点了导入——所以那两处展示是本设计的承重部分，不可省略。
    // 用户在应用内手动配置的脚本不走这条路径。
    let enabled = request.usage_enabled.unwrap_or(false);

    let usage_script = UsageScript {
        enabled,
        language: "javascript".to_string(),
        code,
        timeout: Some(10),
        api_key: usage_api_key_override(request),
        base_url: usage_base_url_override(request),
        access_token: request.usage_access_token.clone(),
        user_id: request.usage_user_id.clone(),
        template_type: None, // Deeplink providers don't specify template type (will use backward compatibility logic)
        auto_query_interval: request.usage_auto_interval,
        coding_plan_provider: None,
        access_key_id: None,
        secret_access_key: None,
        team_organization_id: None,
        team_project_id: None,
    };

    Ok(Some(ProviderMeta {
        usage_script: Some(usage_script),
        ..Default::default()
    }))
}

/// Build Claude settings configuration
///
/// Merges env from the inline config (if any) with the standard fields from URL params.
/// URL params take priority — they overwrite same-named fields from the config.
/// Non-standard env fields (e.g. `ANTHROPIC_CUSTOM_HEADERS`, `API_TIMEOUT_MS`,
/// `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS`, ...) are preserved as-is so that
/// providers requiring extra environment variables work after deeplink import.
fn build_claude_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    // Start from the full env block in the inline config (if present), so any
    // custom env vars the user passed via `config=<base64-json>` survive the
    // import. Falling back to an empty map keeps the previous behavior for
    // deeplinks that don't carry a config field.
    let mut env = extract_claude_config_env(request).unwrap_or_default();

    // Now overwrite / fill in the standard fields from URL params. URL params
    // are authoritative because they're what the deeplink builder put on the
    // wire — for Claude these are: ANTHROPIC_AUTH_TOKEN, ANTHROPIC_BASE_URL,
    // ANTHROPIC_MODEL, and the haiku/sonnet/opus model aliases.
    env.insert(
        "ANTHROPIC_AUTH_TOKEN".to_string(),
        json!(request.api_key.clone().unwrap_or_default()),
    );
    env.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        json!(get_primary_endpoint(request)),
    );

    let slots = claude_slots(request);
    // Add default model if provided
    if let Some(model) = slots.model.as_ref() {
        env.insert("ANTHROPIC_MODEL".to_string(), json!(model));
    }

    // Add Claude-specific model fields (v3.7.1+)
    if let Some(haiku_model) = slots.haiku_model.as_ref() {
        env.insert(
            "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
            json!(haiku_model),
        );
    }
    if let Some(sonnet_model) = slots.sonnet_model.as_ref() {
        env.insert(
            "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
            json!(sonnet_model),
        );
    }
    if let Some(opus_model) = slots.opus_model.as_ref() {
        env.insert(
            "ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(),
            json!(opus_model),
        );
    }

    json!({ "env": env })
}

/// Fill all Claude model slots from the group whitelist when the link only
/// carried a primary model.
///
/// Claude Code routes the main loop, planning and background calls through
/// different aliases. A configuration that only sets `ANTHROPIC_MODEL` works
/// until the first sub-agent call, then fails with a 404 for the missing Haiku
/// or Opus slot. Explicit link parameters always win.
#[derive(Default)]
struct ClaudeSlots {
    model: Option<String>,
    sonnet_model: Option<String>,
    opus_model: Option<String>,
    haiku_model: Option<String>,
}

fn claude_slots(request: &DeepLinkImportRequest) -> ClaudeSlots {
    let mut slots = ClaudeSlots {
        model: request.model.clone(),
        sonnet_model: request.sonnet_model.clone(),
        opus_model: request.opus_model.clone(),
        haiku_model: request.haiku_model.clone(),
    };
    let Some(models) = request.models.as_ref().filter(|models| !models.is_empty()) else {
        return slots;
    };

    let newest = |prefix: &str| {
        models
            .iter()
            .filter(|model| model.starts_with(prefix))
            .max_by(|a, b| version_key(a).cmp(&version_key(b)))
            .cloned()
    };
    let sonnet = newest("claude-sonnet");
    let opus = newest("claude-opus");
    let haiku = newest("claude-haiku");
    let fallback = sonnet
        .clone()
        .or_else(|| opus.clone())
        .or_else(|| request.model.clone())
        .or_else(|| models.first().cloned());

    slots.model = slots.model.or_else(|| fallback.clone());
    slots.sonnet_model = slots
        .sonnet_model
        .or_else(|| sonnet.or_else(|| fallback.clone()));
    slots.opus_model = slots
        .opus_model
        .or_else(|| opus.or_else(|| fallback.clone()));
    slots.haiku_model = slots
        .haiku_model
        .or_else(|| haiku.or_else(|| fallback.clone()));
    slots
}

fn version_key(model: &str) -> (u64, u64, u64) {
    let digits = model
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter(|part| !part.is_empty())
        .find_map(|part| {
            let mut pieces = part
                .split('.')
                .map(|piece| piece.parse::<u64>().unwrap_or(0));
            Some((
                pieces.next().unwrap_or(0),
                pieces.next().unwrap_or(0),
                pieces.next().unwrap_or(0),
            ))
        });
    digits.unwrap_or((0, 0, 0))
}

/// Which Claude Desktop mode a one-click import should produce.
///
/// Claude Desktop's 3P profile validates `inferenceModels` as a whole: one name
/// that is not `claude-*` (or `anthropic/claude-*`) makes it drop the entire
/// list, leaving the model menu empty — see `is_claude_safe_model_id` and the
/// `catalogSafeRouteId` repair path in `claude_desktop_config`.
///
/// A group that sells Claude models therefore maps cleanly onto **direct** mode:
/// the group's own ids go into the profile, and Desktop talks to the relay
/// itself. A group selling DeepSeek / GLM / Kimi has no valid Desktop model id at
/// all, so direct mode is not merely suboptimal, it cannot work. Those imports
/// become **proxy** providers: Desktop sees synthesised `claude-*` route ids and
/// the local router maps them to the real models.
///
/// The deciding question is therefore "does the group carry any Claude model",
/// not "is this a Claude group" — a mixed group still gets direct mode, because
/// its Claude ids are what Desktop will use.
fn desktop_mode_for_request(request: &DeepLinkImportRequest) -> ClaudeDesktopMode {
    // An explicit choice wins. The in-app 「接入」 button sets this; a console link
    // does not, and falls through to the inference below.
    match request
        .claude_desktop_mode
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("proxy") => return ClaudeDesktopMode::Proxy,
        Some("direct") => return ClaudeDesktopMode::Direct,
        // Anything else is not a mode we know; fall through rather than guess.
        _ => {}
    }

    let models: Vec<&String> = request
        .models
        .iter()
        .flatten()
        .chain(request.model.iter())
        .collect();
    let has_claude = models.iter().any(|model| {
        let lower = model.trim().to_ascii_lowercase();
        lower.starts_with("claude-") || lower.starts_with("anthropic/claude-")
    });

    if has_claude {
        ClaudeDesktopMode::Direct
    } else {
        ClaudeDesktopMode::Proxy
    }
}

/// Model routes for a proxy-mode Claude Desktop provider.
///
/// Route ids must be valid Claude Desktop role names. Desktop's fail-all
/// validator accepts `sonnet` / `opus` / `haiku` / `fable` prefixes followed by a
/// concrete version (see `is_claude_safe_model_id`), so a group of DeepSeek and
/// GLM models is mapped onto those three roles in order, with the upstream model
/// id preserved as `labelOverride` so the menu still says what the model is.
///
/// The alias names deliberately carry a version suffix: a bare `claude-sonnet`
/// is rejected, and reusing Desktop's own current route ids
/// (`claude-sonnet-5`) would collide with the built-in catalog.
/// Identity route map for direct mode.
///
/// In direct mode Desktop talks to the gateway itself, so the "route id" it shows
/// must be a real model id from the group. This builds a one-to-one map of those
/// ids — no renaming, no role synthesis — which is what `inferenceModels` is built
/// from.
///
/// Only `claude-*` names are accepted, because Desktop's validator rejects the
/// whole list on a single foreign name: one `deepseek-*` entry would empty the menu
/// for every model.
fn desktop_direct_routes(
    request: &DeepLinkImportRequest,
) -> std::collections::HashMap<String, crate::provider::ClaudeDesktopModelRoute> {
    use crate::provider::ClaudeDesktopModelRoute;

    let mut ordered: Vec<String> = request.models.clone().unwrap_or_default();
    if let Some(primary) = request
        .model
        .clone()
        .filter(|value| !value.trim().is_empty())
    {
        ordered.retain(|id| id != &primary);
        ordered.insert(0, primary);
    }

    let mut routes = std::collections::HashMap::new();
    for id in ordered {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        if !(lower.starts_with("claude-") || lower.starts_with("anthropic/claude-")) {
            // Skip rather than fail: one foreign name must not empty the menu.
            continue;
        }
        routes.insert(
            trimmed.to_string(),
            ClaudeDesktopModelRoute {
                model: trimmed.to_string(),
                label_override: None,
                supports_1m: None,
            },
        );
    }
    routes
}

fn desktop_proxy_routes(
    request: &DeepLinkImportRequest,
) -> std::collections::HashMap<String, crate::provider::ClaudeDesktopModelRoute> {
    use crate::provider::ClaudeDesktopModelRoute;

    // Newest-first within a family, and families interleaved, so the three role
    // slots get the three strongest models rather than three DeepSeek variants.
    let mut ordered: Vec<String> = request.models.clone().unwrap_or_default();
    if let Some(primary) = request
        .model
        .clone()
        .filter(|value| !value.trim().is_empty())
    {
        ordered.retain(|id| id != &primary);
        ordered.insert(0, primary);
    }
    ordered.retain(|id| {
        let lower = id.to_ascii_lowercase();
        !lower.contains("embedding")
            && !lower.contains("rerank")
            && !lower.contains("whisper")
            && !lower.contains("image")
    });
    ordered.dedup();
    if ordered.is_empty() {
        return std::collections::HashMap::new();
    }

    const ROLE_ROUTE_IDS: [&str; 4] = [
        "claude-sonnet-aimyria",
        "claude-opus-aimyria",
        "claude-haiku-aimyria",
        "claude-fable-aimyria",
    ];

    let mut routes = std::collections::HashMap::new();
    for (index, upstream) in ordered.iter().enumerate() {
        // More models than roles: the extras reuse the sonnet slot under a
        // suffixed route id so nothing is silently dropped.
        let route_id = match ROLE_ROUTE_IDS.get(index) {
            Some(id) => (*id).to_string(),
            None => format!("claude-sonnet-aimyria-{index}"),
        };
        routes.insert(
            route_id,
            ClaudeDesktopModelRoute {
                model: upstream.clone(),
                label_override: Some(upstream.clone()),
                supports_1m: None,
            },
        );
    }
    routes
}

/// Decode and extract the `env` object from the deeplink's inline config payload.
///
/// Returns `None` when no config is attached, when the payload can't be
/// decoded/parsed, or when it doesn't contain a Claude-style `env` object.
/// This is a best-effort accessor — we deliberately don't surface parse
/// errors here because `parse_and_merge_config` will have already validated
/// the payload during the merge phase; any failure at this point just means
/// "fall back to URL-param-only behavior".
fn extract_claude_config_env(
    request: &DeepLinkImportRequest,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    // Only the inline base64 config carries an env block. Remote config_url
    // is not implemented yet (see parse_and_merge_config), so nothing else to
    // try here.
    let config_b64 = request.config.as_ref()?;

    // Honor the declared format; default to JSON like parse_and_merge_config does.
    let format = request.config_format.as_deref().unwrap_or("json");
    if format != "json" {
        // Claude config is always JSON in practice. TOML/other formats aren't
        // expected on this app path, so don't try to handle them — safer to
        // fall back than to risk silently producing the wrong shape.
        return None;
    }

    // Decode the base64 payload. We re-decode here rather than threading the
    // already-decoded value through every build_* function, because the call
    // graph (parse_and_merge_config is pub and called separately for preview)
    // makes signature changes invasive. Decode cost is negligible on this
    // one-shot import path.
    let decoded = decode_base64_param("config", config_b64).ok()?;
    let json_str = std::str::from_utf8(&decoded).ok()?;
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;

    // Pull out the env object — same shape as Claude's own settings.json.
    value.get("env").and_then(|v| v.as_object()).cloned()
}

/// Build Codex settings configuration
fn build_codex_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    let provider_display_name = request
        .name
        .as_deref()
        .unwrap_or("custom")
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    let provider_display_name = if provider_display_name.is_empty() {
        "custom".to_string()
    } else {
        provider_display_name
    };

    // Model name: use deeplink model or default
    let model_name = request
        .model
        .as_deref()
        .unwrap_or("gpt-5-codex")
        .to_string();

    // Endpoint: normalize trailing slashes (use primary endpoint only)
    let endpoint = get_primary_endpoint(request)
        .trim()
        .trim_end_matches('/')
        .to_string();

    let provider_display_name = toml_edit::Value::from(provider_display_name.as_str()).to_string();
    let model_name = toml_edit::Value::from(model_name.as_str()).to_string();
    let endpoint = toml_edit::Value::from(endpoint.as_str()).to_string();

    // Build config.toml content
    let config_toml = format!(
        r#"model_provider = "custom"
model = {model_name}
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = {provider_display_name}
base_url = {endpoint}
wire_api = "responses"
requires_openai_auth = true
"#
    );

    json!({
        "auth": {
            "OPENAI_API_KEY": request.api_key,
        },
        "config": config_toml
    })
}

/// Build Gemini settings configuration
fn build_gemini_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    let mut env = serde_json::Map::new();
    env.insert("GEMINI_API_KEY".to_string(), json!(request.api_key));
    env.insert(
        "GOOGLE_GEMINI_BASE_URL".to_string(),
        json!(get_primary_endpoint(request)),
    );

    // Add model if provided
    if let Some(model) = &request.model {
        env.insert("GEMINI_MODEL".to_string(), json!(model));
    }

    json!({ "env": env })
}

fn build_grokbuild_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    let model = request
        .model
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(crate::grok_config::DEFAULT_MODEL)
        .trim();
    let name = request
        .name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("custom")
        .trim();
    let endpoint = get_primary_endpoint(request).trim().to_string();
    let api_key = request.api_key.as_deref().unwrap_or("").trim();

    let model_value = toml_edit::Value::from(model).to_string();
    let name_value = toml_edit::Value::from(name).to_string();
    let endpoint_value = toml_edit::Value::from(endpoint.as_str()).to_string();
    let api_key_value = toml_edit::Value::from(api_key).to_string();

    json!({
        "config": format!(
            "[models]\ndefault = {model_value}\n\n[model.{model_value}]\nmodel = {model_value}\nbase_url = {endpoint_value}\nname = {name_value}\napi_key = {api_key_value}\napi_backend = \"{}\"\ncontext_window = {}\n",
            crate::grok_config::DEFAULT_API_BACKEND,
            crate::grok_config::DEFAULT_CONTEXT_WINDOW,
        )
    })
}

/// Build OpenCode settings configuration
fn build_opencode_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    let endpoint = get_primary_endpoint(request);

    // Build options object
    let mut options = serde_json::Map::new();
    if !endpoint.is_empty() {
        options.insert("baseURL".to_string(), json!(endpoint));
    }
    if let Some(api_key) = &request.api_key {
        options.insert("apiKey".to_string(), json!(api_key));
    }

    // Build models object.
    //
    // A new provider ID cannot inherit an existing one's definition — the write
    // boundary rejects an empty `models`, which is why an OpenCode import used
    // to fail outright when the link carried no model. So the model is never
    // empty: the link's `model` wins, and the default below is what a one-click
    // link with no model falls back to.
    let mut models = serde_json::Map::new();
    let ids = request
        .models
        .clone()
        .filter(|models| !models.is_empty())
        .unwrap_or_else(|| {
            vec![request
                .model
                .clone()
                .unwrap_or_else(|| AIMYRIA_OPENCODE_DEFAULT_MODEL.to_string())]
        });
    for model in ids {
        models.insert(model.clone(), json!({ "name": model }));
    }

    // npm: the link may override; otherwise the openai-compatible package, which
    // is what an OpenAI-shaped relay endpoint speaks.
    let npm = request
        .npm
        .as_deref()
        .unwrap_or(AIMYRIA_OPENCODE_DEFAULT_NPM);

    json!({
        "npm": npm,
        "options": options,
        "models": models
    })
}

/// The npm package an OpenCode provider uses unless the deep link names another.
///
/// Not a product guess: `@ai-sdk/openai-compatible` is what this function already
/// hardcoded before the link was able to override it.
const AIMYRIA_OPENCODE_DEFAULT_NPM: &str = "@ai-sdk/openai-compatible";

/// The model a one-click link configures OpenCode with when it names none.
///
/// A choice made here so that one-click works without a website change — the
/// current links carry no model. It has to be a single constant because the whole
/// point of a default is that it is changed in one place.
const AIMYRIA_OPENCODE_DEFAULT_MODEL: &str = "gpt-5";

/// Build settings for OpenClaw (camelCase live config).
/// Format: { baseUrl, apiKey, api, models }
fn build_additive_app_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    let endpoint = get_primary_endpoint(request);

    let mut config = serde_json::Map::new();

    if !endpoint.is_empty() {
        config.insert("baseUrl".to_string(), json!(endpoint));
    }

    if let Some(api_key) = &request.api_key {
        config.insert("apiKey".to_string(), json!(api_key));
    }

    config.insert("api".to_string(), json!("openai-completions"));

    let models = request.models.clone().unwrap_or_else(|| {
        request
            .model
            .clone()
            .map(|model| vec![model])
            .unwrap_or_default()
    });
    if !models.is_empty() {
        config.insert(
            "models".to_string(),
            Value::Array(
                models
                    .iter()
                    .map(|model| json!({ "id": model, "name": model }))
                    .collect(),
            ),
        );
    }

    json!(config)
}

/// Build Hermes provider settings (snake_case YAML-native fields).
///
/// Hermes' `custom_providers:` entries use `base_url` / `api_key` / `api_mode`
/// (see `_VALID_CUSTOM_PROVIDER_FIELDS` in upstream `hermes_cli/config.py`).
/// Emitting camelCase here — as the OpenClaw path does — would poison the
/// YAML with unknown root fields the Hermes runtime ignores.
///
/// `api_mode` is always written explicitly. Deeplinks have no field to carry
/// it, so we default to `chat_completions` (the most widely compatible
/// protocol) and let the user adjust via the UI after import. We never rely
/// on Hermes' built-in URL heuristics, which only recognize a handful of
/// official endpoints.
fn build_hermes_settings(request: &DeepLinkImportRequest) -> serde_json::Value {
    let endpoint = get_primary_endpoint(request);

    let mut config = serde_json::Map::new();

    if let Some(name) = request.name.as_deref().filter(|s| !s.is_empty()) {
        config.insert("name".to_string(), json!(name));
    }

    if !endpoint.is_empty() {
        config.insert("base_url".to_string(), json!(endpoint));
    }

    if let Some(api_key) = &request.api_key {
        config.insert("api_key".to_string(), json!(api_key));
    }

    config.insert("api_mode".to_string(), json!("chat_completions"));

    let models = request.models.clone().unwrap_or_else(|| {
        request
            .model
            .clone()
            .map(|model| vec![model])
            .unwrap_or_default()
    });
    if !models.is_empty() {
        config.insert(
            "models".to_string(),
            Value::Array(
                models
                    .iter()
                    .map(|model| json!({ "id": model, "name": model }))
                    .collect(),
            ),
        );
    }

    json!(config)
}

// =============================================================================
// Config Merge Logic
// =============================================================================

/// Parse and merge configuration from Base64 encoded config or remote URL
///
/// Priority: URL params > inline config > remote config
pub fn parse_and_merge_config(
    request: &DeepLinkImportRequest,
) -> Result<DeepLinkImportRequest, AppError> {
    // If no config provided, return original request
    if request.config.is_none() && request.config_url.is_none() {
        return Ok(request.clone());
    }

    // Step 1: Get config content
    let config_content = if let Some(config_b64) = &request.config {
        // Decode Base64 inline config
        let decoded = decode_base64_param("config", config_b64)?;
        String::from_utf8(decoded)
            .map_err(|e| AppError::InvalidInput(format!("Invalid UTF-8 in config: {e}")))?
    } else if let Some(_config_url) = &request.config_url {
        // Fetch remote config (TODO: implement remote fetching in next phase)
        return Err(AppError::InvalidInput(
            "Remote config URL is not yet supported. Use inline config instead.".to_string(),
        ));
    } else {
        return Ok(request.clone());
    };

    // Step 2: Parse config based on format
    let format = request.config_format.as_deref().unwrap_or("json");
    let config_value: serde_json::Value = match format {
        "json" => serde_json::from_str(&config_content)
            .map_err(|e| AppError::InvalidInput(format!("Invalid JSON config: {e}")))?,
        "toml" => {
            let toml_value: toml::Value = toml::from_str(&config_content)
                .map_err(|e| AppError::InvalidInput(format!("Invalid TOML config: {e}")))?;
            // Convert TOML to JSON for uniform processing
            serde_json::to_value(toml_value)
                .map_err(|e| AppError::Message(format!("Failed to convert TOML to JSON: {e}")))?
        }
        _ => {
            return Err(AppError::InvalidInput(format!(
                "Unsupported config format: {format}"
            )))
        }
    };

    let mut merged = request.clone();

    // Step 3: Extract values from config based on app type and merge with URL params

    // MCP, Skill and other resource types don't need config merging
    if request.resource != "provider" {
        return Ok(merged);
    }

    match request.app.as_deref().unwrap_or("") {
        "claude" => merge_claude_config(&mut merged, &config_value)?,
        "codex" => merge_codex_config(&mut merged, &config_value)?,
        "gemini" => merge_gemini_config(&mut merged, &config_value)?,
        "grokbuild" => merge_grokbuild_config(&mut merged, &config_value)?,
        // Additive mode apps use JSON config directly; pass through as-is
        "openclaw" | "opencode" | "hermes" | "workbuddy" | "dsh" => {
            merge_additive_config(&mut merged, &config_value)?;
        }
        "" => {
            // No app specified, skip merging
            return Ok(merged);
        }
        _ => {
            return Err(AppError::InvalidInput(format!(
                "Invalid app type: {:?}",
                request.app
            )))
        }
    }

    Ok(merged)
}

/// Merge Claude configuration from config file
fn merge_claude_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    let env = config
        .get("env")
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            AppError::InvalidInput("Claude config must have 'env' object".to_string())
        })?;

    // Auto-fill API key if not provided in URL
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(token) = env.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()) {
            request.api_key = Some(token.to_string());
        }
    }

    // Auto-fill endpoint if not provided in URL
    if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(base_url) = env.get("ANTHROPIC_BASE_URL").and_then(|v| v.as_str()) {
            request.endpoint = Some(base_url.to_string());
        }
    }

    // Auto-fill homepage from endpoint if not provided
    if request.homepage.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(endpoint) = request.endpoint.as_ref().filter(|s| !s.is_empty()) {
            request.homepage = infer_homepage_from_endpoint(endpoint);
            if request.homepage.is_none() {
                request.homepage = Some("https://anthropic.com".to_string());
            }
        }
    }

    // Auto-fill model fields (URL params take priority)
    if request.model.is_none() {
        request.model = env
            .get("ANTHROPIC_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if request.haiku_model.is_none() {
        request.haiku_model = env
            .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if request.sonnet_model.is_none() {
        request.sonnet_model = env
            .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if request.opus_model.is_none() {
        request.opus_model = env
            .get("ANTHROPIC_DEFAULT_OPUS_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }

    Ok(())
}

/// Merge Codex configuration from config file
fn merge_codex_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    // Auto-fill API key from auth.OPENAI_API_KEY or Codex mobile-compatible bearer token.
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        let config_str = config.get("config").and_then(|v| v.as_str());
        if let Some(api_key) =
            crate::codex_config::extract_codex_api_key(config.get("auth"), config_str)
        {
            request.api_key = Some(api_key.to_string());
        }
    }

    // Auto-fill endpoint and model from config string
    if let Some(config_str) = config.get("config").and_then(|v| v.as_str()) {
        // Parse TOML config string to extract base_url and model
        if let Ok(toml_value) = toml::from_str::<toml::Value>(config_str) {
            // Extract base_url from model_providers section
            if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) {
                if let Some(base_url) = extract_codex_base_url(&toml_value) {
                    request.endpoint = Some(base_url);
                }
            }

            // Extract model
            if request.model.is_none() {
                if let Some(model) = toml_value.get("model").and_then(|v| v.as_str()) {
                    request.model = Some(model.to_string());
                }
            }
        }
    }

    // Auto-fill homepage from endpoint
    if request.homepage.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(endpoint) = request.endpoint.as_ref().filter(|s| !s.is_empty()) {
            request.homepage = infer_homepage_from_endpoint(endpoint);
            if request.homepage.is_none() {
                request.homepage = Some("https://openai.com".to_string());
            }
        }
    }

    Ok(())
}

/// Merge Gemini configuration from config file
fn merge_gemini_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    // Gemini uses flat env structure
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(api_key) = config.get("GEMINI_API_KEY").and_then(|v| v.as_str()) {
            request.api_key = Some(api_key.to_string());
        }
    }

    if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(base_url) = config
            .get("GOOGLE_GEMINI_BASE_URL")
            .or_else(|| config.get("GEMINI_BASE_URL"))
            .and_then(|v| v.as_str())
        {
            request.endpoint = Some(base_url.to_string());
        }
    }

    if request.model.is_none() {
        request.model = config
            .get("GEMINI_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }

    // Auto-fill homepage from endpoint
    if request.homepage.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(endpoint) = request.endpoint.as_ref().filter(|s| !s.is_empty()) {
            request.homepage = infer_homepage_from_endpoint(endpoint);
            if request.homepage.is_none() {
                request.homepage = Some("https://ai.google.dev".to_string());
            }
        }
    }

    Ok(())
}

fn merge_grokbuild_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    let config_toml = if let Some(config_toml) = config.get("config").and_then(|v| v.as_str()) {
        config_toml.to_string()
    } else {
        let toml_value: toml::Value = serde_json::from_value(config.clone()).map_err(|error| {
            AppError::InvalidInput(format!("Invalid Grok Build config: {error}"))
        })?;
        toml::to_string(&toml_value).map_err(|error| {
            AppError::InvalidInput(format!("Invalid Grok Build config: {error}"))
        })?
    };
    let model = crate::grok_config::extract_model_config(&config_toml).ok_or_else(|| {
        AppError::InvalidInput("Invalid Grok Build config.toml model profile".to_string())
    })?;

    if request
        .api_key
        .as_ref()
        .is_none_or(|value| value.is_empty())
    {
        // Only inline an explicitly-declared `api_key`. Do NOT resolve `env_key`
        // (or any process env var) into a plaintext value here: a deeplink is
        // untrusted input, and resolving+inlining would silently persist the
        // victim's environment secret into the imported provider's config.toml
        // and ship it to whatever `base_url` the link declares. `env_key` is an
        // indirection that must stay a name, not a resolved secret, on import.
        request.api_key = model.api_key;

        // An `env_key`-only link is not importable at all, and saying so beats
        // falling through to the generic "API key is required" (which reads like
        // a malformed link and invites a "just carry the name over" fix).
        // Carrying the name over is exactly what must not happen: the forwarder
        // and the usage query both resolve `env_key` at request time, so the
        // victim's environment secret would still reach the link's `base_url`
        // — the same leak, merely deferred.
        if request
            .api_key
            .as_ref()
            .is_none_or(|value| value.is_empty())
            && model.env_key.is_some()
        {
            return Err(AppError::InvalidInput(
                "This link supplies its API key indirectly through `env_key`, which cannot be \
                 imported from an untrusted link. Add the provider manually and enter the key \
                 yourself."
                    .to_string(),
            ));
        }
    }
    if request
        .endpoint
        .as_ref()
        .is_none_or(|value| value.is_empty())
    {
        request.endpoint = Some(model.base_url);
    }
    if request.model.is_none() {
        request.model = Some(model.model);
    }
    if request
        .homepage
        .as_ref()
        .is_none_or(|value| value.is_empty())
    {
        if let Some(endpoint) = request.endpoint.as_deref() {
            request.homepage = infer_homepage_from_endpoint(endpoint);
        }
    }

    Ok(())
}

/// Merge configuration for additive mode apps (OpenClaw, OpenCode)
///
/// These apps use JSON config directly, so we only extract common fields
/// (api_key, endpoint, model) from the config if not already set in URL params.
fn merge_additive_config(
    request: &mut DeepLinkImportRequest,
    config: &serde_json::Value,
) -> Result<(), AppError> {
    // Extract api_key from config if not provided in URL
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(api_key) = config
            .get("apiKey")
            .or_else(|| config.get("api_key"))
            .and_then(|v| v.as_str())
        {
            request.api_key = Some(api_key.to_string());
        }
    }

    // Extract endpoint from config if not provided in URL
    if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(base_url) = config
            .get("baseUrl")
            .or_else(|| config.get("base_url"))
            .or_else(|| config.get("options").and_then(|o| o.get("baseURL")))
            .and_then(|v| v.as_str())
        {
            request.endpoint = Some(base_url.to_string());
        }
    }

    // Auto-fill homepage from endpoint
    if request.homepage.as_ref().is_none_or(|s| s.is_empty()) {
        if let Some(endpoint) = request.endpoint.as_ref().filter(|s| !s.is_empty()) {
            request.homepage = infer_homepage_from_endpoint(endpoint);
        }
    }

    Ok(())
}

/// Extract base_url from Codex TOML config
fn extract_codex_base_url(toml_value: &toml::Value) -> Option<String> {
    // Try to find base_url in model_providers section
    if let Some(providers) = toml_value.get("model_providers").and_then(|v| v.as_table()) {
        for (_key, provider) in providers.iter() {
            if let Some(base_url) = provider.get("base_url").and_then(|v| v.as_str()) {
                return Some(base_url.to_string());
            }
        }
    }
    None
}
