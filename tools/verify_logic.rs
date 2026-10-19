//! Standalone verification of the pure logic in `live_writer` and
//! `commands::aimyria`.
//!
//! ## Why this exists
//!
//! `cargo test --lib` links the whole Tauri app, which on this MinGW-gnu setup
//! fails at load time with `STATUS_ENTRYPOINT_NOT_FOUND` on `WebView2Loader.dll`
//! — a toolchain/packaging problem, not a logic problem. To keep verifying the
//! logic while that is sorted out, the pure functions are compiled standalone
//! with `rustc` and exercised here.
//!
//! Run with:
//!   cd src-tauri && cargo test --test verify_logic
//!
//! A separate cargo target (declared in Cargo.toml) rather than a bare
//! `rustc --test` invocation, because this file uses serde_json. Handing rustc
//! an `--extern serde_json=<rlib>` path picks one of several rlibs left behind by
//! different rebuilds, and the wrong one fails to link with a message naming only
//! transitive rlibs. Cargo resolves it.
//!
//! The function bodies below are copies. When one changes in the crate, copy it
//! again — this file is a harness, not the source of truth.

// ── copied from src/live_writer.rs ────────────────────────────────────────

pub const GEMINI_OWNED_KEYS: &[&str] = &["GOOGLE_GEMINI_BASE_URL", "GEMINI_API_KEY"];

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

const PENDING_KEY: &str = "aimyria-editor-pending-key";

fn find_pending(value: &serde_json::Value) -> Option<String> {
    use serde_json::Value;
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

// ── copied from src/commands/aimyria.rs ───────────────────────────────────

const LOW_BALANCE_USD: f64 = 2.0;
const RELAY_HOST: &str = "api.aimyria.com";

#[derive(Default, Debug, PartialEq)]
struct RelayBalance {
    remaining: Option<f64>,
    low: bool,
}

fn low_flag(remaining: Option<f64>) -> bool {
    remaining.is_some_and(|r| r < LOW_BALANCE_USD)
}

fn points_at_aimyria(text: &str) -> bool {
    text.contains(RELAY_HOST)
}

// ── copied from src/aimyria_apps.rs (write helpers) ───────────────────────

const PROVIDER_KEY: &str = "aimyria";

fn remove_marked_block(existing: &str, begin: &str, end: &str) -> String {
    let mut output = String::new();
    let mut inside = false;
    for line in existing.lines() {
        if line.trim() == begin {
            inside = true;
            continue;
        }
        if line.trim() == end {
            inside = false;
            continue;
        }
        if !inside {
            output.push_str(line);
            output.push('\n');
        }
    }
    output
}

fn dsh_patch_block(endpoint: &str, models: &[String], primary: &str) -> String {
    let model_yaml = if models.is_empty() {
        format!("          - id: {primary}\n            name: {primary}\n")
    } else {
        models
            .iter()
            .map(|id| format!("          - id: {id}\n            name: {id}\n"))
            .collect::<String>()
    };
    format!(
        "# aimyria:begin\n\
- id: llm-pi-ai\n\
  config:\n\
    providers:\n\
      aimyria:\n\
        displayName: Aimyria\n\
        apiKeyEnv: AIMYRIA_API_KEY\n\
        api: openai-completions\n\
        baseURL: {endpoint}\n\
        models:\n\
{model_yaml}\
- id: llm-deepseek\n\
  disabled: true\n\
- id: agent-default-model\n\
  config:\n\
    provider: aimyria\n\
    model: {primary}\n\
# aimyria:end\n"
    )
}

fn workbuddy_models_after(existing: &str, entry: serde_json::Value) -> serde_json::Value {
    let mut root: serde_json::Value =
        serde_json::from_str(existing).unwrap_or_else(|_| serde_json::json!({}));
    let object = root.as_object_mut().expect("object");
    let mut models = object
        .remove("models")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    models.retain(|row| row.get("id").and_then(|v| v.as_str()) != Some(PROVIDER_KEY));
    models.insert(0, entry);
    object.insert("models".to_string(), serde_json::Value::Array(models));
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── dotenv merge ────────────────────────────────────────────────

    #[test]
    fn a_fresh_env_file_gets_both_assignments() {
        let owned = [
            ("GOOGLE_GEMINI_BASE_URL", "https://api.aimyria.com"),
            ("GEMINI_API_KEY", "sk-1"),
        ];
        assert_eq!(
            upsert_env_lines("", &owned),
            "GOOGLE_GEMINI_BASE_URL=https://api.aimyria.com\nGEMINI_API_KEY=sk-1\n"
        );
    }

    #[test]
    fn an_existing_key_is_replaced_in_place() {
        let existing = "# my notes\nGEMINI_API_KEY=old-key\nOTHER=keep\n";
        let out = upsert_env_lines(existing, &[("GEMINI_API_KEY", "sk-new")]);
        assert_eq!(out, "# my notes\nGEMINI_API_KEY=sk-new\nOTHER=keep\n");
        assert_eq!(out.matches("GEMINI_API_KEY").count(), 1);
    }

    #[test]
    fn comments_and_foreign_keys_survive() {
        let existing = "# top\nFOO=bar\n\n  # indented comment\nBAZ=qux\n";
        let out = upsert_env_lines(existing, &[("GEMINI_API_KEY", "sk-1")]);
        assert!(out.contains("# top"));
        assert!(out.contains("FOO=bar"));
        assert!(out.contains("  # indented comment"));
        assert!(out.contains("BAZ=qux"));
        assert!(out.ends_with("GEMINI_API_KEY=sk-1\n"));
    }

    #[test]
    fn an_export_prefix_line_is_preserved_rather_than_corrupted() {
        let out = upsert_env_lines(
            "export GEMINI_API_KEY=old\n",
            &[("GEMINI_API_KEY", "sk-new")],
        );
        assert_eq!(out, "export GEMINI_API_KEY=old\nGEMINI_API_KEY=sk-new\n");
    }

    #[test]
    fn a_replacement_run_is_idempotent() {
        let owned = [
            ("GEMINI_API_KEY", "sk-1"),
            ("GOOGLE_GEMINI_BASE_URL", "https://api.aimyria.com"),
        ];
        let once = upsert_env_lines("# keep me\n", &owned);
        let twice = upsert_env_lines(&once, &owned);
        assert_eq!(once, twice, "re-running 一键配置 must not churn the file");
    }

    #[test]
    fn a_missing_trailing_newline_is_normalised() {
        let out = upsert_env_lines("FOO=bar", &[("GEMINI_API_KEY", "k")]);
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn assignment_detection_ignores_comments_and_blanks() {
        assert_eq!(env_assignment("# KEY=v"), None);
        assert_eq!(env_assignment("   "), None);
        assert_eq!(env_assignment("NO_EQUALS"), None);
        assert_eq!(env_assignment("  KEY = v"), Some("KEY"));
        assert_eq!(env_assignment("="), None);
    }

    // ── placeholder rejection ───────────────────────────────────────

    #[test]
    fn pending_key_is_detected_at_any_depth() {
        assert_eq!(
            find_pending(&json!({ "env": { "ANTHROPIC_AUTH_TOKEN": PENDING_KEY } })).as_deref(),
            Some("env.ANTHROPIC_AUTH_TOKEN")
        );
        assert_eq!(
            find_pending(&json!({ "auth": { "OPENAI_API_KEY": PENDING_KEY } })).as_deref(),
            Some("auth.OPENAI_API_KEY")
        );
    }

    #[test]
    fn a_filled_row_has_no_pending_key() {
        assert!(find_pending(&json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "sk-real" } })).is_none());
    }

    // ── balance threshold ───────────────────────────────────────────

    #[test]
    fn an_unknown_balance_never_reads_as_low() {
        assert!(!low_flag(None));
    }

    #[test]
    fn the_threshold_itself_is_not_low() {
        assert!(!low_flag(Some(LOW_BALANCE_USD)));
    }

    #[test]
    fn just_under_the_threshold_is_low() {
        assert!(low_flag(Some(LOW_BALANCE_USD - 0.01)));
    }

    #[test]
    fn a_healthy_balance_is_not_low() {
        assert!(!low_flag(Some(50.0)));
    }

    // ── configured detection ────────────────────────────────────────

    #[test]
    fn a_config_pointing_elsewhere_is_not_configured() {
        assert!(!points_at_aimyria(
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://elsewhere.example"}}"#
        ));
    }

    #[test]
    fn a_config_pointing_at_us_is_configured() {
        assert!(points_at_aimyria(
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://api.aimyria.com"}}"#
        ));
    }

    #[test]
    fn the_relay_host_is_url_shaped() {
        assert!(RELAY_HOST.starts_with("api."));
        assert!(!RELAY_HOST.contains(' '));
        assert_eq!(
            RelayBalance::default(),
            RelayBalance {
                remaining: None,
                low: false
            }
        );
    }

    // ── Aimyria-only writers ────────────────────────────────────────

    #[test]
    fn workbuddy_keeps_hand_written_models_and_replaces_ours() {
        let root = workbuddy_models_after(
            r#"{"models":[{"id":"mine","name":"Mine"},{"id":"aimyria","name":"Old"}]}"#,
            json!({"id": "aimyria", "name": "New"}),
        );
        let models = root["models"].as_array().unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["name"], "New");
        assert_eq!(models[1]["id"], "mine");
    }

    #[test]
    fn dsh_block_is_idempotent_and_preserves_other_patches() {
        let existing = "- id: user-plugin\n  config:\n    keep: true\n";
        let block = dsh_patch_block(
            "https://api.aimyria.com/v1",
            &["deepseek-v4.1-flash".to_string(), "glm-5.3".to_string()],
            "deepseek-v4.1-flash",
        );
        let once = format!("{existing}{block}");
        let twice = format!(
            "{}{}",
            remove_marked_block(&once, "# aimyria:begin", "# aimyria:end"),
            block
        );
        assert!(twice.contains("user-plugin"));
        assert_eq!(twice.matches("# aimyria:begin").count(), 1);
        // The model appears once as `id` and once as `name` inside its entry.
        assert_eq!(twice.matches("deepseek-v4.1-flash").count(), 3);
        assert_eq!(twice.matches("- id: llm-pi-ai").count(), 1);
    }
}
