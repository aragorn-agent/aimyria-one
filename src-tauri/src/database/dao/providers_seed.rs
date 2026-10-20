//! Official-provider seeds — the one-click way back to each vendor's own account.
//!
//! # Why these exist
//!
//! A user who has their own ChatGPT / Claude / Google subscription should be able
//! to point the client back at that account without hand-editing config files.
//! That is what these rows are: one card per vendor, carrying **empty**
//! credentials, so switching to it removes what we wrote and lets the client fall
//! back to its own native login (see `services/provider/*_direct.rs`).
//!
//! # Why the empty `settings_config_json`
//!
//! The emptiness is the feature, not an oversight. Claude/Claude Desktop seed
//! `{"env":{}}` — no floor keys, so the projection clears ours and the CLI's own
//! credential store takes over. Codex seeds `{"auth":{},"config":""}` — empty
//! auth plus empty TOML, so ChatGPT Plus/Pro OAuth is what runs. Gemini seeds
//! `{"env":{},"config":{}}` for Google OAuth, and Grok an empty `config` so its
//! built-in xAI OAuth login is used. A seed with real credentials would instead
//! pin the user to whatever key we shipped, which is the opposite of the point.
//!
//! # Why this file is not allowed to go empty again
//!
//! It was empty once, on purpose, with the reasoning that an official row is a
//! "price-shopping surface": a user who can see "OpenAI Official" one click away
//! from us might leave. That is a product judgment, not a technical one — and it
//! had a real cost, because removing the data left every downstream consumer
//! silently broken: `ensure_official_seed_by_id` could only ever return
//! "unknown official seed", the three `ensure_*` commands always failed, and the
//! Grok official preset button errored on click. Nothing crashed and nothing was
//! logged loudly, so it looked like a feature that simply was not there.
//!
//! The lesson kept in `tools/check_brand.mjs` rather than here: if the seeds are
//! emptied again, that is a product decision that must be made explicitly, and
//! the build gate should say so rather than let the breakage be silent.
//!
//! These entries are **vendor official endpoints only** — each `website_url`
//! points at its own vendor (`anthropic.com`, `chatgpt.com`, `ai.google.dev`,
//! `x.ai`). Third-party relay presets are what the brand gate forbids, and that
//! distinction is the rule now enforced there.

use crate::app_config::AppType;

/// Claude Code, pointed back at Anthropic's own endpoint.
pub(crate) const CLAUDE_OFFICIAL_PROVIDER_ID: &str = "claude-official";
pub(crate) const CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID: &str = "claude-desktop-official";
pub(crate) const CODEX_OFFICIAL_PROVIDER_ID: &str = "codex-official";
pub(crate) const GEMINI_OFFICIAL_PROVIDER_ID: &str = "gemini-official";
pub(crate) const GROKBUILD_OFFICIAL_PROVIDER_ID: &str = "grokbuild-official";

/// One seeded provider row.
pub(crate) struct OfficialProviderSeed {
    pub id: &'static str,
    pub app_type: AppType,
    pub name: &'static str,
    pub website_url: &'static str,
    pub icon: &'static str,
    pub icon_color: &'static str,
    /// `settings_config` as a JSON string; the shape differs per client, and the
    /// empty-credential shape is what lets the client's native login take over.
    pub settings_config_json: &'static str,
}

// `AppType` has no const constructor for its variants, so the list is a `const`
// of a plain struct array rather than anything built at run time. It is the one
// place that says which apps have an official card.
pub(crate) const OFFICIAL_SEEDS: &[OfficialProviderSeed] = &[
    OfficialProviderSeed {
        id: CLAUDE_OFFICIAL_PROVIDER_ID,
        app_type: AppType::Claude,
        name: "Claude Official",
        website_url: "https://www.anthropic.com/claude-code",
        icon: "anthropic",
        icon_color: "#D4915D",
        // 空 env：切换时清掉我们写入的 ANTHROPIC_BASE_URL/token，回落 Claude CLI 自带的登录态。
        settings_config_json: r#"{"env":{}}"#,
    },
    OfficialProviderSeed {
        id: CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
        app_type: AppType::ClaudeDesktop,
        name: "Claude Desktop Official",
        website_url: "https://claude.ai/download",
        icon: "anthropic",
        icon_color: "#D4915D",
        // 空 env 只是占位；切换该 provider 时会把 Claude Desktop 恢复成 1P 模式。
        settings_config_json: r#"{"env":{}}"#,
    },
    OfficialProviderSeed {
        id: CODEX_OFFICIAL_PROVIDER_ID,
        app_type: AppType::Codex,
        name: "OpenAI Official",
        website_url: "https://chatgpt.com/codex",
        icon: "openai",
        icon_color: "#00A67E",
        // 空 auth + 空 config：让 Codex 走 ChatGPT Plus/Pro 的 OAuth 登录。
        settings_config_json: r#"{"auth":{},"config":""}"#,
    },
    OfficialProviderSeed {
        id: GEMINI_OFFICIAL_PROVIDER_ID,
        app_type: AppType::Gemini,
        name: "Google Official",
        website_url: "https://ai.google.dev/",
        icon: "gemini",
        icon_color: "#4285F4",
        // 空 env + 空 config：让 Gemini CLI 走 Google OAuth 登录。
        settings_config_json: r#"{"env":{},"config":{}}"#,
    },
    OfficialProviderSeed {
        id: GROKBUILD_OFFICIAL_PROVIDER_ID,
        app_type: AppType::GrokBuild,
        name: "Grok Official",
        website_url: "https://x.ai/grok",
        icon: "grok",
        icon_color: "currentColor",
        // 空 config = 不写自定义模型表，Grok CLI 回落到自带的 xAI OAuth 登录。
        settings_config_json: r#"{"config":""}"#,
    },
];

/// Whether `id` belongs to a built-in official seed.
///
/// Guards "do not treat a seeded row as a user row": migrations skip these ids,
/// and the live-import path refuses to overwrite them.
pub(crate) fn is_official_seed_id(id: &str) -> bool {
    OFFICIAL_SEEDS.iter().any(|seed| seed.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ids the rest of the crate references by constant must actually be in
    /// the list. They were once kept "so the referencing code compiles" while the
    /// list was empty — which meant every call site was dead. This asserts the
    /// two cannot drift apart again.
    #[test]
    fn every_referenced_id_is_seeded() {
        for id in [
            CLAUDE_OFFICIAL_PROVIDER_ID,
            CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
            CODEX_OFFICIAL_PROVIDER_ID,
            GEMINI_OFFICIAL_PROVIDER_ID,
            GROKBUILD_OFFICIAL_PROVIDER_ID,
        ] {
            assert!(
                OFFICIAL_SEEDS.iter().any(|seed| seed.id == id),
                "{id} is referenced as an official seed but is not in OFFICIAL_SEEDS"
            );
            assert!(is_official_seed_id(id));
        }
    }

    /// The membership test must answer "no" for anything not in the list —
    /// otherwise a user row could be mistaken for a seeded one and protected
    /// from deletion.
    #[test]
    fn unrelated_ids_are_not_official() {
        for id in ["", "my-provider", "aimyria", "claude-official-2"] {
            assert!(!is_official_seed_id(id), "{id} must not count as a seed");
        }
    }

    /// Each seed's settings_config_json must be valid JSON, since the seed path
    /// parses it and a malformed literal would fail at first launch rather than
    /// at build time.
    #[test]
    fn settings_config_json_is_valid() {
        for seed in OFFICIAL_SEEDS {
            let parsed: serde_json::Value = serde_json::from_str(seed.settings_config_json)
                .unwrap_or_else(|e| panic!("seed {} has invalid JSON: {e}", seed.id));
            assert!(
                parsed.is_object(),
                "seed {} settings_config must be an object",
                seed.id
            );
        }
    }

    /// Official seeds must point at the vendors themselves, never at a relay.
    /// This is the product line: going back to your own account is supported,
    /// going to a competitor's relay is not.
    #[test]
    fn official_seeds_point_at_their_own_vendor() {
        let vendor_hosts = [
            "anthropic.com",
            "claude.ai",
            "claude.com",
            "chatgpt.com",
            "openai.com",
            "ai.google.dev",
            "google.com",
            "x.ai",
        ];
        for seed in OFFICIAL_SEEDS {
            // 取 host 而不是整串：官方域名带 `www.` 前缀（`www.anthropic.com`），
            // 用整串 starts_with 会因为那个前缀把它误判成「非本厂商」。
            let after_scheme = seed
                .website_url
                .split_once("://")
                .map(|(_, rest)| rest)
                .unwrap_or(seed.website_url);
            let host = after_scheme
                .split(['/', '?', '#'])
                .next()
                .unwrap_or("")
                .trim_start_matches("www.");
            assert!(
                vendor_hosts.iter().any(|v| host == *v || host.ends_with(&format!(".{v}"))),
                "seed {} points at host '{host}', which is not one of its vendor's own hosts",
                seed.id
            );
        }
    }
}
