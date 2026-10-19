//! Official-provider seeds — empty on purpose.
//!
//! # Why this file still exists
//!
//! Upstream seeded five rows on every fresh database: Claude Official, Claude
//! Desktop Official, OpenAI Official, Google Official and Grok Official, so a
//! user always had a one-click way back to each vendor's own endpoint.
//!
//! Aimyria One has exactly one provider: us. Those seeds are not a neutral
//! convenience here — they are a price-shopping surface. A user who can see
//! "OpenAI Official" one click away from "Aimyria" will use it, and then the
//! tool has failed at the only thing it does.
//!
//! So the list is empty and `is_official_seed_id` matches nothing.
//! `Database::init_default_official_providers` stays, and stays idempotent:
//! it walks an empty list and does nothing. That keeps the call site in
//! `Database::init` working without special-casing the removal, which keeps the
//! diff small and reversible.
//!
//! # What replaces it
//!
//! The Aimyria row itself, written by `ProviderService::add` from a deep link
//! or from the relay console. Nothing is seeded locally, because a seeded row
//! would have no credentials and would sit in the list as a dead entry.

use crate::app_config::AppType;

// Retained so `crate::database` re-exports and call sites keep compiling. No
// seed uses them; they are here only until the referencing code is removed.
pub(crate) const CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID: &str = "claude-desktop-official";
pub(crate) const CODEX_OFFICIAL_PROVIDER_ID: &str = "codex-official";
pub(crate) const GROKBUILD_OFFICIAL_PROVIDER_ID: &str = "grokbuild-official";

/// One seeded provider row.
pub(crate) struct OfficialProviderSeed {
    pub id: &'static str,
    pub app_type: AppType,
    pub name: &'static str,
    pub website_url: &'static str,
    pub icon: &'static str,
    pub icon_color: &'static str,
    /// `settings_config` as a JSON string; the shape differs per client.
    pub settings_config_json: &'static str,
}

/// Deliberately empty. See the module docs.
pub(crate) const OFFICIAL_SEEDS: &[OfficialProviderSeed] = &[];

/// Whether `id` belongs to a built-in official seed.
///
/// Always false here. Kept as a function rather than deleted so the callers
/// that guard "do not touch a seeded row" keep working and start returning
/// true-for-nothing instead of needing edits.
pub(crate) fn is_official_seed_id(_id: &str) -> bool {
    false
}
