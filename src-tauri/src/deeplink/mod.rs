//! Deep link import functionality for AIMYRIA GO
//!
//! This module implements the aimyria:// protocol for importing configurations
//! via deep links: providers, MCP servers, prompts and skills.
//!
//! The provider path is ours — `parser.rs` / `provider.rs` / `utils.rs` are the
//! files the 5-of-5-clients regression was verified against, and they were restored
//! after a wholesale copy of upstream's `deeplink/` directory broke it. Upstream's
//! `mcp.rs`, `prompt.rs` and `skill.rs` are kept for the three extra import kinds.

mod mcp;
mod parser;
mod prompt;
pub(crate) mod provider;
mod skill;
mod utils;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// The most recent deep link that arrived before any window could receive it.
///
/// # Why this exists
///
/// `handle_deeplink_url` emits `deeplink-import` and then shows the window. On a
/// cold start — the app not yet running, which is the *normal* state on a
/// freshly installed machine and right after sign-in — that emit happens during
/// `setup()`, before the webview has loaded and before React has mounted the
/// dialog's `listen()`. Tauri broadcasts events; an event with no listener is
/// dropped, not queued. The window then appears with no confirmation dialog, and
/// the user is left looking at a screen that asks them nothing.
///
/// That is the worst possible outcome for this product: a deep link is the only
/// way an account gets bound, so a lost one is a silently dead install.
///
/// The stash is the compensation. The request is recorded here *before* the
/// emit, and the frontend claims it once it is actually listening. Claiming is
/// destructive, so a request is never shown twice.
///
/// Only the latest one is kept: a deep link is an instruction about what to do
/// now, and replaying a stale one after the user has moved on would be worse than
/// dropping it. A newer link overwrites an unclaimed older one.
static PENDING_DEEPLINK: Mutex<Option<DeepLinkImportRequest>> = Mutex::new(None);

/// Record a deep link for a listener that is not mounted yet.
pub fn stash_pending_deeplink(request: &DeepLinkImportRequest) {
    match PENDING_DEEPLINK.lock() {
        Ok(mut slot) => *slot = Some(request.clone()),
        // A poisoned lock means another thread panicked while holding it. The
        // stash is a convenience, never a correctness requirement for the write
        // path, so recovering the value is better than taking the process down.
        Err(poisoned) => *poisoned.into_inner() = Some(request.clone()),
    }
}

/// Take the stashed deep link, if any. Destructive: the caller owns it now.
pub fn take_pending_deeplink() -> Option<DeepLinkImportRequest> {
    match PENDING_DEEPLINK.lock() {
        Ok(mut slot) => slot.take(),
        Err(poisoned) => poisoned.into_inner().take(),
    }
}

/// Is a deep link waiting to be claimed? Non-destructive.
///
/// The self-updater uses this as one of its "a user is mid-import" signals: a
/// stashed-but-unclaimed request means the confirmation dialog has not been shown
/// yet, so restarting the process now would lose the only instruction that binds
/// an account. Peeking is what makes that check possible without destroying the
/// request it is asking about.
pub fn has_pending_deeplink() -> bool {
    match PENDING_DEEPLINK.lock() {
        Ok(slot) => slot.is_some(),
        Err(poisoned) => poisoned.into_inner().is_some(),
    }
}

// Re-export public API
pub use mcp::import_mcp_from_deeplink;
pub use parser::parse_deeplink_url;
pub use prompt::import_prompt_from_deeplink;
pub use provider::{import_provider_from_deeplink, merge_group_models, parse_and_merge_config};
pub use skill::import_skill_from_deeplink;

/// Deep link import request model
///
/// Represents a parsed aimyria:// URL ready for processing.
/// This struct contains all possible fields for all resource types.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepLinkImportRequest {
    /// Protocol version (e.g., "v1")
    pub version: String,
    /// Resource type to import: "provider" | "prompt" | "mcp" | "skill"
    pub resource: String,

    // ============ Common fields ============
    /// Target application (claude/codex/gemini) - for provider, prompt, skill
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// Resource name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whether to enable after import (default: false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    // ============ Provider-specific fields ============
    /// Provider homepage URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// API endpoint/base URL (supports comma-separated multiple URLs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// API key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Optional provider icon name (maps to built-in SVG)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Optional model name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional npm package for OpenCode (overrides the built-in default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm: Option<String>,
    /// Optional notes/description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Optional Haiku model (Claude only, v3.7.1+)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub haiku_model: Option<String>,
    /// Optional Sonnet model (Claude only, v3.7.1+)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sonnet_model: Option<String>,
    /// Optional Opus model (Claude only, v3.7.1+)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opus_model: Option<String>,

    // ============ Aimyria relay fields ============
    /// Every model the key's group owns. The website fills this after reading
    /// `/api/v1/ops/group-models`; the desktop uses it to configure all slots
    /// without a second network round-trip.
    ///
    /// Serialized as a comma-separated string in the URL (`models=a,b,c`) and
    /// as a JSON array between the backend and frontend. The website builds
    /// the string form because query parameters cannot express a JSON array
    /// without URL-encoding every bracket.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_models",
        serialize_with = "serialize_models"
    )]
    pub models: Option<Vec<String>>,
    /// Human-readable group name, shown in the import confirmation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
    /// A short-lived, read-only balance token issued by the relay console.
    ///
    /// This is deliberately separate from the API key: the key can call models,
    /// but it cannot be used to read an account profile without turning the
    /// relay's dashboard auth into an API-key endpoint. A token lets the
    /// desktop show the balance without storing the user's login session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance_token: Option<String>,
    /// Read-only balance endpoint override, for deployments where the bridge
    /// lives on the main site rather than the model gateway.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance_url: Option<String>,
    /// Explicit Claude Desktop routing mode: `"direct"` or `"proxy"`.
    ///
    /// Left `None` the mode is inferred from the model list — a group serving real
    /// `claude-*` ids is imported direct, anything else as a proxy (local router),
    /// because Desktop's validator rejects foreign model names outright.
    ///
    /// That inference suits a console link, which knows nothing about intent. It
    /// does **not** suit the in-app 「接入」 button: a user who picks proxy mode has
    /// said what they want, and re-deciding from a model name would silently
    /// override them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_desktop_mode: Option<String>,
    /// Original aimyria:// URL, attached when the OS delivered the link.
    ///
    /// Not parsed from the query string; it is the whole request. The UI shows
    /// it only for diagnostics and lets a user copy the same import to another
    /// machine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_url: Option<String>,

    // ============ Prompt-specific fields ============
    /// Base64 encoded Markdown content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Prompt description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    // ============ MCP-specific fields ============
    /// Target applications for MCP (comma-separated: "claude,codex,gemini")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<String>,

    // ============ Skill-specific fields ============
    /// GitHub repository (format: "owner/name")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// Skill directory name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,

    // ============ Config file fields (v3.8+) ============
    /// Base64 encoded config content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    /// Config format (json/toml)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_format: Option<String>,
    /// Remote config URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_url: Option<String>,

    // ============ Usage script fields (v3.9+) ============
    /// Whether to enable usage query. Defaults to **disabled** — carrying a script
    /// is not itself a decision to run it; the link must say `usageEnabled=true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_enabled: Option<bool>,
    /// Base64 encoded usage query script code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_script: Option<String>,
    /// Usage query API key (if different from provider API key)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_api_key: Option<String>,
    /// Usage query base URL (if different from provider endpoint)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_base_url: Option<String>,
    /// Usage query access token (for NewAPI template)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_access_token: Option<String>,
    /// Usage query user ID (for NewAPI template)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_user_id: Option<String>,
    /// Auto query interval in minutes (0 to disable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_auto_interval: Option<u64>,
}

/// Accept both the JSON array the frontend sends between commands and the
/// comma-separated form a URL query uses.
fn deserialize_models<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Array(Vec<String>),
        Text(String),
    }
    let raw = Option::<Raw>::deserialize(deserializer)?;
    Ok(raw.map(|value| match value {
        Raw::Array(models) => models,
        Raw::Text(text) => text
            .split(',')
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_string)
            .collect(),
    }))
}

fn serialize_models<S>(models: &Option<Vec<String>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match models {
        Some(models) => serializer.serialize_some(models),
        None => serializer.serialize_none(),
    }
}
