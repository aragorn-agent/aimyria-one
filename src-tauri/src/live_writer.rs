//! Minimal live-config writer for Aimyria One.
//!
//! # Why this module exists
//!
//! Upstream CC Switch routed every client write through its `mode/` engine —
//! roughly 10k lines that maintained a pending-operation ledger, rollback
//! bookkeeping, local-proxy takeover and multi-provider "Stack" merging.
//! Aimyria One has exactly one provider and no proxy, so none of that applies.
//! This module replaces the whole engine with a direct, atomic write.
//!
//! # Semantics
//!
//! - Read the client's existing file when it parses; otherwise start from empty.
//! - Overwrite only the keys the provider row owns.
//! - Keep keys that exist in the file but not in the provider row, so a user's
//!   own unrelated settings survive a switch.
//! - Write through a temp file + rename, so a crash mid-write cannot leave a
//!   truncated config behind.
//!
//! # Clients
//!
//! | Client | File(s) written | Shape |
//! |---|---|---|
//! | Claude Code | `~/.claude/settings.json` | `{"env": {...}}` merged |
//! | Codex | `~/.codex/auth.json`, `~/.codex/config.toml` | `{"auth":…, "config":…}` |
//! | Gemini CLI | `~/.gemini/.env` | dotenv lines, layout preserved |
//! | OpenCode | `~/.config/opencode/opencode.json` | `provider.aimyria` |
//! | OpenClaw | `~/.openclaw/openclaw.json` | `models.providers.aimyria` |
//!
//! The path helpers each client module exposes already own the "where does this
//! live" question, including per-app directory overrides, so this module never
//! re-derives a path.
//!
//! # Deviations from upstream, deliberately
//!
//! Upstream stored a pending-operation ledger so an interrupted switch could be
//! replayed on next launch. We do not: if the write fails we surface the error
//! and leave the previous config in place. With one provider there is nothing to
//! reconcile on the next launch.
//!
//! Upstream wrote a placeholder API key into the live config when the user had
//! not filled one in yet, then restored the old key on the next successful
//! switch. We refuse the write instead — see [`ensure_ready`]. Overwriting a
//! working key with a placeholder would break a working client, and refusing is
//! something the user can understand and fix.

use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::app_config::AppType;
use crate::config::{atomic_write_private, get_claude_settings_path, read_json_file};
use crate::error::AppError;

/// Placeholder stored in a provider row while the user has not filled the API
/// key in yet. A row containing this value is never written to a live config.
pub const PENDING_KEY: &str = "aimyria-editor-pending-key";

/// The single provider id Aimyria One writes under.
///
/// Every client we support is configured from one relay, so there is exactly
/// one entry to add and exactly one to replace on a re-run. Naming it keeps a
/// user's own hand-written provider entries in the same file untouched.
pub const PROVIDER_ID: &str = "aimyria";

/// The clients Aimyria One can configure, in the order the UI shows them.
///
/// `None` for `cli` means "we can write the config, but we do not need the
/// binary on PATH to do it" — the config file is the contract, not the binary.
pub struct ClientSpec {
    pub app_type: AppType,
    /// Human label for the UI.
    pub label: &'static str,
    /// Executable name to look for on `PATH`, when the client ships one.
    pub cli: Option<&'static str>,
}

/// Every client this build can write.
///
/// `GrokBuild`, `Hermes`, `Pi` and `Mcode` are upstream clients we do not
/// configure: writing a config for a tool we cannot support would fail at the
/// worst moment, so they are absent rather than half-wired.
///
/// `ClaudeDesktop` was listed as absent for the same reason, and that reason was
/// wrong. It is not half-wired — `claude_desktop_config` is 2,000-odd lines with
/// `apply_provider`, `get_status`, `provider_mode` and the gateway token, and
/// `services/provider/live.rs` handles `AppType::ClaudeDesktop` on read, on write
/// and on the additive-mode check. It was simply never added to this list, so a
/// one-click link configured five of the six clients and skipped the one the owner
/// was looking at.
pub const CLIENTS: &[ClientSpec] = &[
    ClientSpec {
        app_type: AppType::Claude,
        label: "Claude Code",
        cli: Some("claude"),
    },
    ClientSpec {
        app_type: AppType::ClaudeDesktop,
        label: "Claude Desktop",
        // The desktop app ships no CLI on PATH.
        cli: None,
    },
    ClientSpec {
        app_type: AppType::Codex,
        label: "Codex",
        cli: Some("codex"),
    },
    ClientSpec {
        app_type: AppType::Gemini,
        label: "Gemini CLI",
        cli: Some("gemini"),
    },
    ClientSpec {
        app_type: AppType::OpenCode,
        label: "OpenCode",
        cli: Some("opencode"),
    },
    ClientSpec {
        app_type: AppType::OpenClaw,
        label: "OpenClaw",
        cli: Some("openclaw"),
    },
];

/// Look up a client spec by its [`AppType`].
pub fn client_spec(app_type: &AppType) -> Option<&'static ClientSpec> {
    CLIENTS.iter().find(|spec| &spec.app_type == app_type)
}

/// Path of the file this module writes for a given app.
///
/// The editor layer needs the path to show which file a change will land in.
pub fn live_path(app_type: &AppType) -> Option<PathBuf> {
    match app_type {
        AppType::Claude => Some(get_claude_settings_path()),
        AppType::Codex => Some(crate::codex_config::get_codex_config_path()),
        AppType::Gemini => Some(crate::gemini_config::get_gemini_env_path()),
        AppType::OpenCode => crate::opencode_config::get_opencode_config_path().ok(),
        AppType::OpenClaw => Some(crate::openclaw_config::get_openclaw_config_path()),
        // Claude Desktop's relay address does NOT live in `claude_desktop_config.json`.
        // That file carries only the deployment-mode patch and is 28 bytes on a fully
        // configured install. The gateway profile — `inferenceGatewayBaseUrl`,
        // `inferenceGatewayApiKey`, `inferenceProvider` — goes to
        // `<configLibrary>/<PROFILE_ID>.json`. `apply_provider` writes four files and
        // that one is the payload; the other three are the deployment patch and meta.
        //
        // Pointing `live_path` at the wrong one of the four is what made the
        // one-click screen say 「该客户端不支持写入」 for a client that was already
        // configured — the status check read a file with no relay in it. Same shape as
        // reading `claude_desktop_config.json` by hand and concluding that nothing had
        // been written: the file to read is the one the writer puts the value in.
        AppType::ClaudeDesktop => crate::claude_desktop_config::get_config_library_path()
            .ok()
            .map(|dir| dir.join(format!("{}.json", crate::claude_desktop_config::PROFILE_ID))),
        _ => None,
    }
}

/// Write `settings` into the client's live configuration.
///
/// `settings` is the provider row's `settings_config` value:
/// - Claude Code: a JSON object merged into `~/.claude/settings.json`
/// - Codex: `{"auth": {...}, "config": "<config.toml text>"}`, written to
///   `~/.codex/auth.json` and `~/.codex/config.toml`
pub fn write(app_type: &AppType, settings: &Value) -> Result<(), AppError> {
    match app_type {
        AppType::Claude => write_claude_code(settings),
        AppType::Codex => write_codex(settings),
        AppType::Gemini => write_gemini(settings),
        AppType::OpenCode => write_opencode(settings),
        AppType::OpenClaw => write_openclaw(settings),
        other => Err(AppError::InvalidInput(format!(
            "{} is not supported by this build yet",
            other.as_str()
        ))),
    }
}

/// Claude Code: merge the provider's keys into `~/.claude/settings.json`.
///
/// Keys the row does not mention are left as they are, so a user's unrelated
/// settings (hooks, permissions, MCP entries) survive a provider switch.
fn write_claude_code(settings: &Value) -> Result<(), AppError> {
    let owned = owned_object(settings);
    let path = get_claude_settings_path();

    let mut merged: Map<String, Value> = match read_json_file::<Value>(&path) {
        Ok(Value::Object(existing)) => existing,
        // A missing or unparseable file is not an error: the row is the source
        // of truth and we are about to write a well-formed file anyway.
        _ => Map::new(),
    };
    for (key, value) in owned {
        merged.insert(key, value);
    }

    let text = serde_json::to_string_pretty(&Value::Object(merged))
        .map_err(|e| AppError::InvalidInput(format!("failed to serialize settings: {e}")))?;
    atomic_write_private(&path, format!("{text}\n").as_bytes())
}

/// Codex: write `auth.json` and `config.toml` together.
///
/// `codex_config::write_codex_live_atomic` already validates the TOML, writes
/// both files with private permissions, and rolls `auth.json` back if
/// `config.toml` fails — so this only has to split the row's two fields.
fn write_codex(settings: &Value) -> Result<(), AppError> {
    let auth = settings
        .get("auth")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let config_text = settings.get("config").and_then(Value::as_str).unwrap_or("");

    crate::codex_config::write_codex_live_atomic(&auth, Some(config_text))
}

/// Env keys this module owns inside `~/.gemini/.env`.
///
/// `GOOGLE_API_KEY` is deliberately absent: it is the legacy Google key, and
/// overwriting it would break a user's direct-to-Google setup. We only set the
/// two keys that point at our relay.
const GEMINI_OWNED_KEYS: &[&str] = &["GOOGLE_GEMINI_BASE_URL", "GEMINI_API_KEY"];

/// Gemini CLI: merge the provider's env into `~/.gemini/.env`.
///
/// Gemini reads a dotenv file, not JSON, so this is a line edit rather than a
/// structural merge. Lines we own are replaced in place; every other line —
/// comments, ordering, the user's own unrelated keys — is preserved verbatim.
fn write_gemini(settings: &Value) -> Result<(), AppError> {
    let env = settings
        .get("env")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::InvalidInput("Gemini 配置缺少 env 段".to_string()))?;

    let owned: Vec<(&str, &str)> = GEMINI_OWNED_KEYS
        .iter()
        .filter_map(|key| env.get(*key).and_then(Value::as_str).map(|v| (*key, v)))
        .collect();
    if owned.is_empty() {
        return Err(AppError::InvalidInput(
            "Gemini 配置里没有可写入的字段".to_string(),
        ));
    }

    let path = crate::gemini_config::get_gemini_env_path();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let updated = upsert_env_lines(&existing, &owned);
    crate::gemini_config::write_gemini_env_text_atomic(&updated)
}

/// OpenCode: write the provider under `provider.aimyria`.
///
/// `opencode_config::set_provider` picks between the legacy `provider` map and
/// the native `providers` map from the row's shape, and rejects a write that
/// would shadow a native declaration. We forward the row unchanged so that
/// decision stays in one place.
fn write_opencode(settings: &Value) -> Result<(), AppError> {
    crate::opencode_config::set_provider(PROVIDER_ID, Value::Object(owned_object(settings)))
}

/// OpenClaw: write the provider under `models.providers.aimyria`.
fn write_openclaw(settings: &Value) -> Result<(), AppError> {
    crate::openclaw_config::set_provider(PROVIDER_ID, Value::Object(owned_object(settings)))
        .map(|_| ())
}

/// Apply `owned` assignments to a dotenv document, preserving everything else.
///
/// A line whose key is ours is replaced; a line that is anything else is copied
/// through untouched. Assignments we did not previously have are appended, so a
/// first run on a fresh machine produces a valid file rather than a partial one.
fn upsert_env_lines<'a>(content: &str, owned: &[(&'a str, &'a str)]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut written: Vec<&str> = Vec::new();

    for line in content.lines() {
        match env_assignment(line) {
            Some(key) => match owned.iter().find(|(k, _)| *k == key) {
                Some((_, value)) => {
                    out.push(format!("{key}={value}"));
                    written.push(key);
                }
                // Not one of ours: the user's own setting, keep it.
                None => out.push(line.to_string()),
            },
            None => out.push(line.to_string()),
        }
    }

    for (key, value) in owned {
        if !written.contains(key) {
            out.push(format!("{key}={value}"));
        }
    }

    let mut text = out.join("\n");
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// The key of a dotenv assignment, or `None` for blanks, comments and lines
/// with no `=`.
fn env_assignment(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let key = trimmed.split_once('=')?.0.trim();
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

/// The provider row's own top-level keys, as an object.
///
/// A non-object row means the row is malformed; treat it as owning nothing
/// rather than panicking, so the caller reports the real error.
fn owned_object(settings: &Value) -> Map<String, Value> {
    match settings {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    }
}

/// Reject a row that still holds a placeholder key.
///
/// Returning an error here is deliberate: see the module docs. The caller
/// surfaces it, the user fills the key in, and the switch is retried.
pub fn ensure_ready(app_type: &AppType, settings: &Value) -> Result<(), AppError> {
    if find_pending(settings).is_some() {
        return Err(AppError::InvalidInput(format!(
            "{} 的 API Key 还是占位符，请先填写真实的 Key 再切换",
            app_type.as_str()
        )));
    }
    Ok(())
}

/// First `key -> value` pair in `value` whose leaf is the placeholder.
///
/// Returns the key path so the error message can name the field to fix.
fn find_pending(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if s == PENDING_KEY => Some(String::new()),
        Value::Object(map) => {
            for (key, child) in map {
                if let Some(rest) = find_pending(child) {
                    return Some(if rest.is_empty() {
                        key.clone()
                    } else {
                        format!("{key}.{rest}")
                    });
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(find_pending),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_key_is_detected_at_any_depth() {
        let direct = serde_json::json!({ "env": { "ANTHROPIC_AUTH_TOKEN": PENDING_KEY } });
        assert_eq!(
            find_pending(&direct).as_deref(),
            Some("env.ANTHROPIC_AUTH_TOKEN")
        );

        let nested = serde_json::json!({ "auth": { "OPENAI_API_KEY": PENDING_KEY } });
        assert_eq!(
            find_pending(&nested).as_deref(),
            Some("auth.OPENAI_API_KEY")
        );
    }

    #[test]
    fn a_filled_row_is_ready() {
        let filled = serde_json::json!({
            "env": { "ANTHROPIC_AUTH_TOKEN": "sk-real-key" }
        });
        assert!(find_pending(&filled).is_none());
        assert!(ensure_ready(&AppType::Claude, &filled).is_ok());
    }

    #[test]
    fn a_placeholder_row_is_rejected() {
        let pending = serde_json::json!({ "auth": { "OPENAI_API_KEY": PENDING_KEY } });
        let err = ensure_ready(&AppType::Codex, &pending).unwrap_err();
        assert!(err.to_string().contains("占位符"), "got: {err}");
    }

    #[test]
    fn unsupported_apps_are_reported_not_silently_skipped() {
        let err = write(&AppType::Pi, &serde_json::json!({})).unwrap_err();
        assert!(err.to_string().contains("not supported"), "got: {err}");
    }

    #[test]
    fn every_listed_client_has_a_writable_path() {
        for spec in CLIENTS {
            assert!(
                live_path(&spec.app_type).is_some(),
                "{} has no live path",
                spec.label
            );
        }
    }

    #[test]
    fn clients_we_do_not_support_have_no_path() {
        for app in [AppType::GrokBuild] {
            assert!(live_path(&app).is_none(), "{}", app.as_str());
            assert!(client_spec(&app).is_none(), "{}", app.as_str());
        }
    }

    // ── dotenv merging ───────────────────────────────────────────────
    //
    // Gemini's `.env` is the one target with no JSON structure to lean on, so
    // the merge is line-based and these are the cases that can go wrong.

    #[test]
    fn a_fresh_env_file_gets_both_assignments() {
        let owned = [
            ("GOOGLE_GEMINI_BASE_URL", "https://api.aimyria.com"),
            ("GEMINI_API_KEY", "sk-1"),
        ];
        let out = upsert_env_lines("", &owned);
        assert_eq!(
            out,
            "GOOGLE_GEMINI_BASE_URL=https://api.aimyria.com\nGEMINI_API_KEY=sk-1\n"
        );
    }

    #[test]
    fn an_existing_key_is_replaced_in_place() {
        let existing = "# my notes\nGEMINI_API_KEY=old-key\nOTHER=keep\n";
        let owned = [("GEMINI_API_KEY", "sk-new")];
        let out = upsert_env_lines(existing, &owned);
        assert_eq!(out, "# my notes\nGEMINI_API_KEY=sk-new\nOTHER=keep\n");
        // No duplicate line left behind.
        assert_eq!(out.matches("GEMINI_API_KEY").count(), 1);
    }

    #[test]
    fn comments_and_foreign_keys_survive() {
        let existing = "# top\nFOO=bar\n\n  # indented comment\nBAZ=qux\n";
        let owned = [("GEMINI_API_KEY", "sk-1")];
        let out = upsert_env_lines(existing, &owned);
        assert!(out.contains("# top"));
        assert!(out.contains("FOO=bar"));
        assert!(out.contains("  # indented comment"));
        assert!(out.contains("BAZ=qux"));
        assert!(out.ends_with("GEMINI_API_KEY=sk-1\n"));
    }

    #[test]
    fn an_export_prefix_is_still_recognised_as_an_assignment() {
        let existing = "export GEMINI_API_KEY=old\n";
        let owned = [("GEMINI_API_KEY", "sk-new")];
        let out = upsert_env_lines(existing, &owned);
        // `export KEY=` parses as key `export GEMINI_API_KEY`, which is not one
        // of ours, so the line is preserved rather than corrupted.
        assert_eq!(out, "export GEMINI_API_KEY=old\nGEMINI_API_KEY=sk-new\n");
    }

    #[test]
    fn assignment_detection_ignores_comments_and_blanks() {
        assert_eq!(env_assignment("# KEY=v"), None);
        assert_eq!(env_assignment("   "), None);
        assert_eq!(env_assignment("NO_EQUALS"), None);
        assert_eq!(env_assignment("  KEY = v"), Some("KEY"));
        assert_eq!(env_assignment("="), None);
    }

    /// The write is re-run every time the user presses 一键配置 and once on
    /// every launch, so a second run must leave the file byte-identical —
    /// otherwise the client reloads a changed config for no reason.
    #[test]
    fn a_replacement_run_is_idempotent() {
        let owned = [
            ("GEMINI_API_KEY", "sk-1"),
            ("GOOGLE_GEMINI_BASE_URL", "https://api.aimyria.com"),
        ];
        let once = upsert_env_lines("# keep me\n", &owned);
        let twice = upsert_env_lines(&once, &owned);
        assert_eq!(once, twice);
    }

    #[test]
    fn a_missing_trailing_newline_is_normalised() {
        let out = upsert_env_lines("FOO=bar", &[("GEMINI_API_KEY", "k")]);
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn gemini_without_an_env_section_is_rejected() {
        let err = write_gemini(&serde_json::json!({ "nope": true })).unwrap_err();
        assert!(err.to_string().contains("env"), "got: {err}");
    }

    #[test]
    fn gemini_with_no_owned_keys_is_rejected_rather_than_writing_an_empty_file() {
        let err = write_gemini(&serde_json::json!({ "env": { "GOOGLE_API_KEY": "legacy" } }))
            .unwrap_err();
        assert!(err.to_string().contains("可写入"), "got: {err}");
    }

    // ── End-to-end, on real files ───────────────────────────────────
    //
    // The tests above exercise pure functions. This one writes to disk and
    // reads the bytes back, because "did the right content land in the right
    // file" is the entire product claim and it is not observable from a return
    // value.
    //
    // One test rather than five: `CC_SWITCH_TEST_HOME` is process-global, and
    // cargo runs tests in parallel. Splitting these would let one case read
    // another's home directory.
    //
    // The temp dir is made unique per run so a stale directory from a previous
    // run — or a crashed one — cannot make a write look like it merged when it
    // actually started from empty.
    #[test]
    fn writes_land_in_the_expected_files_without_clobbering_neighbouring_keys() {
        use std::sync::atomic::{AtomicU32, Ordering};

        static SEQ: AtomicU32 = AtomicU32::new(0);
        let home = std::env::temp_dir().join(format!(
            "aimyria-e2e-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::env::set_var("CC_SWITCH_TEST_HOME", &home);

        let read = |p: std::path::PathBuf| std::fs::read_to_string(&p).unwrap_or_default();

        // ── Claude Code ─────────────────────────────────────────────
        // Pre-seed with a key we do not own. It must survive.
        let claude = home.join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("settings.json"),
            r#"{
  "env": { "MY_OWN_FLAG": "keep-me", "ANTHROPIC_BASE_URL": "https://old.example" },
  "permissions": { "allow": ["Bash(ls:*)"] }
}
"#,
        )
        .unwrap();

        write(
            &AppType::Claude,
            &serde_json::json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.aimyria.com",
                    "ANTHROPIC_AUTH_TOKEN": "sk-test-key"
                }
            }),
        )
        .expect("claude write");

        let text = read(claude.join("settings.json"));
        let parsed: Value = serde_json::from_str(&text).expect("claude settings stay valid JSON");
        assert_eq!(
            parsed["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-test-key",
            "the key must land"
        );
        assert_eq!(
            parsed["env"]["ANTHROPIC_BASE_URL"],
            "https://api.aimyria.com"
        );
        assert_eq!(
            parsed["env"]["MY_OWN_FLAG"], "keep-me",
            "a key we do not own must survive"
        );
        assert!(
            parsed["permissions"]["allow"].is_array(),
            "an unrelated top-level block must survive"
        );

        // ── Codex ───────────────────────────────────────────────────
        write(
            &AppType::Codex,
            &serde_json::json!({
                "auth": { "OPENAI_API_KEY": "sk-codex-key" },
                "config": "model = \"gpt-5\"\nmodel_provider = \"aimyria\"\n"
            }),
        )
        .expect("codex write");

        let auth: Value =
            serde_json::from_str(&read(home.join(".codex").join("auth.json"))).expect("auth.json");
        assert_eq!(auth["OPENAI_API_KEY"], "sk-codex-key");
        let toml = read(home.join(".codex").join("config.toml"));
        assert!(toml.contains("gpt-5"), "config.toml content: {toml}");

        // ── Gemini ──────────────────────────────────────────────────
        let gemini_dir = home.join(".gemini");
        std::fs::create_dir_all(&gemini_dir).unwrap();
        std::fs::write(
            gemini_dir.join(".env"),
            "# hand written\nMY_FLAG=keep-me\nGEMINI_API_KEY=old\n",
        )
        .unwrap();

        write(
            &AppType::Gemini,
            &serde_json::json!({
                "env": {
                    "GOOGLE_GEMINI_BASE_URL": "https://api.aimyria.com",
                    "GEMINI_API_KEY": "sk-gemini-key"
                }
            }),
        )
        .expect("gemini write");

        let env_text = read(gemini_dir.join(".env"));
        assert!(
            env_text.contains("GEMINI_API_KEY=sk-gemini-key"),
            "{env_text}"
        );
        assert!(env_text.contains("GOOGLE_GEMINI_BASE_URL=https://api.aimyria.com"));
        assert!(
            env_text.contains("# hand written"),
            "comment lost: {env_text}"
        );
        assert!(
            env_text.contains("MY_FLAG=keep-me"),
            "foreign key lost: {env_text}"
        );

        // ── Placeholder refusal must leave the file alone ───────────
        let before = read(gemini_dir.join(".env"));
        let err = write(
            &AppType::Claude,
            &serde_json::json!({ "env": { "ANTHROPIC_AUTH_TOKEN": PENDING_KEY } }),
        )
        .unwrap_err();
        assert!(err.to_string().contains("占位符"), "got: {err}");
        assert_eq!(
            read(claude.join("settings.json")),
            text,
            "a refused write must not have touched the file"
        );
        let _ = before;

        // ── Paths point into the test home, not the developer's ──────
        for app in [AppType::Claude, AppType::Codex, AppType::Gemini] {
            let path = live_path(&app).expect("path");
            assert!(
                path.starts_with(&home),
                "{} resolved outside the test home: {}",
                app.as_str(),
                path.display()
            );
        }

        std::env::remove_var("CC_SWITCH_TEST_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }
}
