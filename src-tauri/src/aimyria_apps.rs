//! Aimyria-specific client catalogue and direct writers.
//!
//! `AppType` is CC Switch's enum and stops at the clients upstream knew about.
//! This product also targets Tencent WorkBuddy and DeepSeek Harness (`dsh`),
//! both of which are plain configuration files rather than a new protocol:
//!
//! - WorkBuddy: `~/.workbuddy/models.json` (`%USERPROFILE%` on Windows)
//! - dsh:       `$DSH_HOME/profiles/<profile>/cordis.patch.yml` plus
//!              `$DSH_HOME/.credentials.yaml` (`$DSH_HOME` defaults to `~/.dsh`)
//!
//! They intentionally do **not** get an `AppType` variant: adding one would
//! force changes through the provider database, proxy, tray, session scanner
//! and every upstream page. The one-click importer only needs a settings
//! payload plus a writer, which is exactly what this module provides.
//!
//! Every writer is atomic and only overwrites the entries it owns. Existing
//! WorkBuddy models and unrelated dsh providers are preserved.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::config::{atomic_write, get_home_dir, read_json_file};
use crate::error::AppError;

/// Stable provider key used inside WorkBuddy and dsh.
pub const PROVIDER_KEY: &str = "aimyria";

/// Clients that are not part of [`crate::app_config::AppType`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AimyriaOnlyApp {
    WorkBuddy,
    DeepSeekHarness,
}

impl AimyriaOnlyApp {
    pub fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "workbuddy" | "work-buddy" | "work_buddy" => Some(Self::WorkBuddy),
            "dsh" | "deepseek-harness" | "deepseek_harness" | "deepseekharness" => {
                Some(Self::DeepSeekHarness)
            }
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkBuddy => "workbuddy",
            Self::DeepSeekHarness => "dsh",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::WorkBuddy => "WorkBuddy",
            Self::DeepSeekHarness => "DeepSeek Harness",
        }
    }

    /// The file the UI shows as "which file will be written".
    pub fn live_path(self) -> PathBuf {
        match self {
            Self::WorkBuddy => workbuddy_models_path(),
            Self::DeepSeekHarness => dsh_profile_patch_path(),
        }
    }
}

/// `%USERPROFILE%\.workbuddy\models.json` / `~/.workbuddy/models.json`.
pub fn workbuddy_models_path() -> PathBuf {
    get_home_dir().join(".workbuddy").join("models.json")
}

/// `$DSH_HOME` when set and non-empty, otherwise `~/.dsh`.
pub fn dsh_home() -> PathBuf {
    std::env::var_os("DSH_HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().to_string_lossy().trim().is_empty())
        .unwrap_or_else(|| get_home_dir().join(".dsh"))
}

/// `$DSH_HOME/profiles/default/cordis.patch.yml`.
///
/// dsh profiles are user-created; writing to `default` is the documented
/// shape and keeps the one-click path deterministic. If the user has exactly
/// one profile directory, use it instead so the import lands where dsh is
/// actually running.
pub fn dsh_profile_patch_path() -> PathBuf {
    dsh_home()
        .join("profiles")
        .join(dsh_profile())
        .join("cordis.patch.yml")
}

pub fn dsh_credentials_path() -> PathBuf {
    dsh_home().join(".credentials.yaml")
}

fn dsh_profile() -> String {
    let profiles = dsh_home().join("profiles");
    let mut dirs: Vec<String> = std::fs::read_dir(&profiles)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    dirs.sort();
    if dirs.len() == 1 {
        dirs.remove(0)
    } else {
        "default".to_string()
    }
}

/// Write one provider entry into WorkBuddy's `models.json`.
///
/// WorkBuddy stores an array. We replace the entry with `vendor: "Custom"`
/// and our `id`, preserving every other model the user added by hand.
pub fn write_workbuddy(settings: &Value) -> Result<(), AppError> {
    write_workbuddy_at(&workbuddy_models_path(), settings)
}

pub fn write_workbuddy_at(path: &Path, settings: &Value) -> Result<(), AppError> {
    let mut root = match read_json_file::<Value>(path) {
        Ok(Value::Object(map)) => Value::Object(map),
        _ => json!({}),
    };

    let object = root
        .as_object_mut()
        .ok_or_else(|| AppError::InvalidInput("WorkBuddy 配置必须是 JSON 对象".to_string()))?;
    let mut models = object
        .remove("models")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();

    models.retain(|entry| entry.get("id").and_then(Value::as_str) != Some(PROVIDER_KEY));
    models.insert(0, settings.clone());
    object.insert("models".to_string(), Value::Array(models));

    let text = serde_json::to_string_pretty(&root)
        .map_err(|e| AppError::InvalidInput(format!("WorkBuddy 配置序列化失败: {e}")))?;
    atomic_write(path, format!("{text}\n").as_bytes())
}

/// Write dsh's `cordis.patch.yml` and credentials.
///
/// dsh's patch format is a YAML sequence of plugin patches. We replace our
/// provider block while preserving any other top-level patches, using a
/// marker pair so a re-import is idempotent and a user's own patches survive.
pub fn write_dsh(settings: &Value) -> Result<(), AppError> {
    write_dsh_at(&dsh_profile_patch_path(), &dsh_credentials_path(), settings)
}

pub fn write_dsh_at(
    patch_path: &Path,
    credentials_path: &Path,
    settings: &Value,
) -> Result<(), AppError> {
    let model = settings
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let endpoint = settings
        .get("base_url")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let api_key = settings
        .get("api_key")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let models: Vec<String> = settings
        .get("models")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let block = dsh_patch_block(endpoint, &models, &model);
    let existing = std::fs::read_to_string(patch_path).unwrap_or_default();
    let without_ours = remove_marked_block(&existing, "# aimyria:begin", "# aimyria:end");
    let mut next = without_ours;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&block);
    atomic_write(patch_path, next.as_bytes())?;

    write_dsh_credentials(credentials_path, api_key)
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

fn write_dsh_credentials(path: &Path, api_key: &str) -> Result<(), AppError> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    let mut seen_refs = false;
    let mut seen_key = false;

    for line in existing.lines() {
        if line.trim() == "version: 1" {
            lines.push(line.to_string());
            continue;
        }
        if line.trim() == "refs:" {
            lines.push(line.to_string());
            lines.push(format!("  AIMYRIA_API_KEY: {api_key}"));
            seen_refs = true;
            seen_key = true;
            continue;
        }
        if seen_refs && line.starts_with("  AIMYRIA_API_KEY:") {
            continue;
        }
        lines.push(line.to_string());
    }

    if !seen_key {
        if lines.is_empty() {
            lines.push("version: 1".to_string());
        }
        if !seen_refs {
            lines.push("refs:".to_string());
        }
        lines.push(format!("  AIMYRIA_API_KEY: {api_key}"));
        if !lines.iter().any(|line| line.trim() == "records: {}") {
            lines.push("records: {}".to_string());
        }
    }

    let mut text = lines.join("\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    atomic_write(path, text.as_bytes())
}

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

/// Read the newest model list from the relay's group-models endpoint.
///
/// The endpoint is public at `aimyria.com` and takes only the API key. It is
/// the same data source the website's quick-config panel uses, so the desktop
/// and browser cannot disagree about which models a key owns.
pub async fn fetch_group_models(relay_base: &str, api_key: &str) -> Result<GroupModels, String> {
    let base = relay_base.trim_end_matches('/');
    let url = format!(
        "{}/api/v1/ops/group-models?key={}",
        base,
        urlencoding(api_key)
    );
    let resp = crate::proxy::http_client::get()
        .get(url)
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(12))
        .send()
        .await
        .map_err(|e| format!("读取分组模型失败: {e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取分组模型响应失败: {e}"))?;
    if !status.is_success() {
        return Err(format!("读取分组模型失败（HTTP {status}）: {text}"));
    }
    let body: Value =
        serde_json::from_str(&text).map_err(|e| format!("分组模型响应不是 JSON: {e}"))?;
    let group = body
        .get("data")
        .and_then(|data| data.get("groups"))
        .and_then(Value::as_array)
        .and_then(|groups| groups.first())
        .cloned()
        .ok_or_else(|| "分组模型响应里没有 groups[0]".to_string())?;

    let models = group
        .get("models")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .or_else(|| item.get("id").and_then(Value::as_str).map(str::to_string))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(GroupModels {
        group_name: group
            .get("group_name")
            .or_else(|| group.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        platform: group
            .get("platform")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        models,
    })
}

#[derive(Debug, Clone, Default)]
pub struct GroupModels {
    pub group_name: String,
    pub platform: String,
    pub models: Vec<String>,
}

fn urlencoding(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Build the settings payload for a WorkBuddy import.
pub fn build_workbuddy_settings(endpoint: &str, api_key: &str, models: &[String]) -> Value {
    let first = models.first().cloned().unwrap_or_default();
    let mut object = Map::new();
    object.insert("id".into(), json!(PROVIDER_KEY));
    object.insert("name".into(), json!("Aimyria"));
    object.insert("vendor".into(), json!("Custom"));
    object.insert("baseUrl".into(), json!(endpoint));
    object.insert("apiKey".into(), json!(api_key));
    object.insert("supportsToolsCall".into(), json!(true));
    object.insert("supportsCustomProtocol".into(), json!(false));
    object.insert("useCustomProtocol".into(), json!(false));
    object.insert("model".into(), json!(first));
    object.insert("models".into(), json!(models));
    Value::Object(object)
}

/// Build the settings payload for a dsh import.
pub fn build_dsh_settings(endpoint: &str, api_key: &str, models: &[String]) -> Value {
    json!({
        "base_url": endpoint,
        "api_key": api_key,
        "api_mode": "chat_completions",
        "model": models.first().cloned().unwrap_or_default(),
        "models": models.iter().map(|id| json!({ "id": id, "name": id })).collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workbuddy_replaces_only_its_own_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        std::fs::write(
            &path,
            r#"{"models":[{"id":"hand-written","name":"Mine"},{"id":"aimyria","name":"Old"}]}"#,
        )
        .unwrap();
        write_workbuddy_at(&path, &json!({"id":"aimyria","name":"New"})).unwrap();
        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let models = value["models"].as_array().unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["name"], "New");
        assert_eq!(models[1]["id"], "hand-written");
    }

    #[test]
    fn dsh_rewrite_is_idempotent_and_preserves_other_patches() {
        let dir = tempfile::tempdir().unwrap();
        let patch = dir.path().join("cordis.patch.yml");
        let credentials = dir.path().join(".credentials.yaml");
        std::fs::write(&patch, "- id: user-plugin\n  config:\n    keep: true\n").unwrap();
        let settings = json!({
            "base_url": "https://api.aimyria.com/v1",
            "api_key": "sk-test",
            "models": [{"id":"deepseek-v4.1-flash"},{"id":"glm-5.3"}]
        });
        write_dsh_at(&patch, &credentials, &settings).unwrap();
        write_dsh_at(&patch, &credentials, &settings).unwrap();
        let text = std::fs::read_to_string(&patch).unwrap();
        assert!(text.contains("user-plugin"));
        assert_eq!(text.matches("# aimyria:begin").count(), 1);
        assert_eq!(text.matches("deepseek-v4.1-flash").count(), 1);
        let creds = std::fs::read_to_string(&credentials).unwrap();
        assert!(creds.contains("AIMYRIA_API_KEY: sk-test"));
    }

    #[test]
    fn only_exact_supported_ids_parse() {
        assert_eq!(
            AimyriaOnlyApp::from_str("workbuddy"),
            Some(AimyriaOnlyApp::WorkBuddy)
        );
        assert_eq!(
            AimyriaOnlyApp::from_str("dsh"),
            Some(AimyriaOnlyApp::DeepSeekHarness)
        );
        assert_eq!(AimyriaOnlyApp::from_str("hermes"), None);
    }
}
