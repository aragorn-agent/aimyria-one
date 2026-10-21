// Acceptance test against copies of the REAL configs found on this machine.
//
// The unit tests in `aimyria_apps` use synthetic fixtures. This one is the other
// half: it copies what WorkBuddy and DeepSeek Harness actually have on disk right
// now, runs the shipping writers over those copies, and asserts the user's own
// data is still there afterwards.
//
// Driven by `AIMYRIA_E2E_FIXTURES` (a directory holding `wb-before.json`,
// `dsh-before.yml`); skipped when unset, so a normal `cargo test` stays hermetic.
//
//   AIMYRIA_E2E_FIXTURES=E:\isolated\realdata cargo test --test acceptance_real_configs

use std::path::PathBuf;

use aimyria_one_lib::aimyria_apps;
use serde_json::{json, Value};

fn fixtures() -> Option<PathBuf> {
    std::env::var_os("AIMYRIA_E2E_FIXTURES").map(PathBuf::from)
}

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// WorkBuddy: the file is a top-level array of per-model records. Six real
/// records pointing at `wb.321ai.cc` must survive, and our import must add one
/// record per model without touching theirs.
#[test]
fn workbuddy_real_config_survives_an_import() {
    let Some(dir) = fixtures() else {
        eprintln!("AIMYRIA_E2E_FIXTURES not set; skipping");
        return;
    };
    let work = tempfile::tempdir().unwrap();
    let models = work.path().join("models.json");
    std::fs::copy(dir.join("wb-before.json"), &models).unwrap();

    let before: Value = serde_json::from_str(&read(&models)).unwrap();
    let before_rows = before.as_array().expect("real file is a top-level array").len();

    let relay = "https://api.aimyria.com/v1";
    aimyria_apps::write_workbuddy_at(
        &models,
        &json!({
            "baseUrl": relay,
            "apiKey": "sk-test",
            "models": ["deepseek-v4.1-flash", "glm-5.3"],
        }),
    )
    .expect("write must succeed on the real array shape");

    let after: Value = serde_json::from_str(&read(&models)).unwrap();
    let rows = after.as_array().expect("array shape must be preserved");

    assert_eq!(
        rows.len(),
        before_rows + 2,
        "all {before_rows} user rows kept, plus one record per imported model"
    );
    // The user's rows point at wb.321ai.cc and sub2.aimyria.com — both are our
    // own hosts, so a host-based match would have deleted them. Identity is the
    // exact endpoint this import writes, so all six survive.
    let our_endpoint_rows = rows.iter().filter(|row| row["url"] == relay).count();
    assert_eq!(
        our_endpoint_rows, 2,
        "only the two records this import wrote point at {relay}"
    );
    assert_eq!(
        rows.len() - our_endpoint_rows,
        before_rows,
        "every pre-existing row must survive"
    );
    // Spot-check the two host families explicitly: one wb.321ai.cc row per user
    // model, and the sub2 row is still present.
    let wb_rows = rows
        .iter()
        .filter(|row| row["url"].as_str().is_some_and(|u| u.contains("wb.321ai.cc")))
        .count();
    assert_eq!(wb_rows, 5, "five rows on wb.321ai.cc, none removed");
    assert!(
        rows.iter()
            .any(|row| row["url"].as_str().is_some_and(|u| u.contains("sub2.aimyria.com"))),
        "the row on sub2.aimyria.com must survive an import against api.aimyria.com"
    );
}

/// DeepSeek Harness: the real profile has an `llm-pi-ai` row with the user's
/// other providers, an `agent-default-model`, and a header comment mentioning
/// `!!js`. All three must be untouched.
#[test]
fn dsh_real_profile_keeps_user_providers_and_js_semantics() {
    let Some(dir) = fixtures() else {
        eprintln!("AIMYRIA_E2E_FIXTURES not set; skipping");
        return;
    };
    let work = tempfile::tempdir().unwrap();
    let patch = work.path().join("cordis.patch.yml");
    let creds = work.path().join(".credentials.yaml");
    std::fs::copy(dir.join("dsh-before.yml"), &patch).unwrap();
    std::fs::copy(dir.join("creds-before.yml"), &creds).unwrap();

    let before = read(&patch);

    aimyria_apps::write_dsh_at(
        &patch,
        &creds,
        &json!({
            "base_url": "https://api.aimyria.com/v1",
            "api_key": "sk-test",
            "model": "deepseek-v4.1-flash",
            "models": [
                {"id": "deepseek-v4.1-flash", "name": "deepseek-v4.1-flash"},
                {"id": "glm-5.3", "name": "glm-5.3"}
            ],
        }),
    )
    .expect("write must succeed on the real profile");

    let after = read(&patch);

    assert!(after.contains("opencode-go"), "user provider must survive");
    assert!(after.contains("Aimyria-wb"), "user provider must survive");
    assert!(
        !after.contains("llm-deepseek"),
        "the native DeepSeek account must not be disabled"
    );
    assert_eq!(
        after.matches("- id: llm-pi-ai").count(),
        1,
        "exactly one llm-pi-ai row"
    );
    assert_eq!(
        after.matches("      aimyria:").count(),
        1,
        "our provider appears exactly once"
    );
    // The header comment quotes `!!js`; a parse-and-reserialise writer drops it.
    assert!(
        after.contains("`!!js` expressions allowed"),
        "the header must be preserved verbatim"
    );
    // The user's own default model stays theirs; moving it is not an import's job.
    assert!(
        after.contains("provider: a"),
        "their agent-default-model must not be repointed: {after}"
    );
    // The user's tuning inside our provider block survives; only the three fields
    // this tool owns are rewritten.
    assert!(
        after.contains("reasoning: high"),
        "their reasoning setting must survive: {after}"
    );
    assert!(
        after.contains("thinkingFormat: deepseek"),
        "their compat block must survive: {after}"
    );
    assert!(
        after.contains("defaultContextWindow: 131072"),
        "their context window must survive: {after}"
    );
    // The relay address is the one field an import is meant to change.
    assert!(
        after.contains("baseURL: https://api.aimyria.com/v1"),
        "the import must point the provider at the imported relay"
    );

    // Everything outside our provider block is byte-identical. The block itself is
    // identified by indentation: its key line, and every deeper line after it.
    let our_indent = before
        .lines()
        .find(|line| line.trim_start().starts_with("aimyria:"))
        .map(|line| line.len() - line.trim_start().len())
        .expect("the real profile has an aimyria provider");
    let mut inside_ours = false;
    let kept: Vec<&str> = before
        .lines()
        .filter(|line| {
            if line.trim().is_empty() {
                return false;
            }
            let indent = line.len() - line.trim_start().len();
            if line.trim_start().starts_with("aimyria:") && indent == our_indent {
                inside_ours = true;
                return false;
            }
            if inside_ours {
                if indent > our_indent {
                    return false;
                }
                inside_ours = false;
            }
            true
        })
        .collect();
    for line in kept {
        assert!(
            after.contains(line),
            "line disappeared from the profile: {line:?}"
        );
    }
}

/// Re-importing must be idempotent on the real profile.
#[test]
fn dsh_real_profile_reimport_is_idempotent() {
    let Some(dir) = fixtures() else {
        eprintln!("AIMYRIA_E2E_FIXTURES not set; skipping");
        return;
    };
    let work = tempfile::tempdir().unwrap();
    let patch = work.path().join("cordis.patch.yml");
    let creds = work.path().join(".credentials.yaml");
    std::fs::copy(dir.join("dsh-before.yml"), &patch).unwrap();
    std::fs::copy(dir.join("creds-before.yml"), &creds).unwrap();

    let settings = json!({
        "base_url": "https://api.aimyria.com/v1",
        "api_key": "sk-test",
        "model": "glm-5.3",
        "models": [{"id": "glm-5.3", "name": "glm-5.3"}],
    });
    aimyria_apps::write_dsh_at(&patch, &creds, &settings).unwrap();
    let once = read(&patch);
    aimyria_apps::write_dsh_at(&patch, &creds, &settings).unwrap();
    let twice = read(&patch);

    assert_eq!(once, twice, "a second identical import must change nothing");
    assert_eq!(
        twice.matches("      aimyria:").count(),
        1,
        "no duplicate provider key"
    );
}
