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

use serde_json::{json, Value};

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

/// `$DSH_HOME/profiles/<profile>/cordis.patch.yml`.
///
/// A dsh install normally has **more than one** profile: this machine has
/// `desktop` and `web` (plus a `node_modules` directory that is not a profile at
/// all). The previous rule — "one profile → use it, otherwise `default`" — hit
/// that common case and wrote `profiles/default/cordis.patch.yml`, a file dsh
/// never boots, so 一键配置 reported success while changing nothing.
///
/// Selection order, most specific first:
///
/// 1. `AIMYRIA_DSH_PROFILE` — explicit override, for a user who knows which
///    profile they run.
/// 2. `desktop` — the Electron app's own profile, which is what a desktop user
///    is actually looking at. The CLI refuses `--profile desktop` precisely
///    because the app owns it, so its existence is a reliable signal.
/// 3. `web` — the `dsh web` surface.
/// 4. The only real profile directory, if exactly one is left.
/// 5. `default` — nothing usable found; create it so a first run still writes
///    somewhere the documented CLI can boot (`dsh --profile default`).
pub fn dsh_profile_patch_path() -> PathBuf {
    dsh_home()
        .join("profiles")
        .join(dsh_profile_name())
        .join("cordis.patch.yml")
}

pub fn dsh_credentials_path() -> PathBuf {
    dsh_home().join(".credentials.yaml")
}

/// Profile directories under `$DSH_HOME/profiles`, sorted.
///
/// `node_modules` is skipped: it sits beside the profiles and would otherwise
/// count as one, which is what made the old "exactly one profile" test fail on
/// every real install.
fn dsh_profiles() -> Vec<String> {
    let profiles = dsh_home().join("profiles");
    let mut dirs: Vec<String> = std::fs::read_dir(&profiles)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name != "node_modules" && !name.starts_with('.'))
        .collect();
    dirs.sort();
    dirs
}

fn dsh_profile_name() -> String {
    if let Some(explicit) = std::env::var_os("AIMYRIA_DSH_PROFILE")
        .map(|value| value.to_string_lossy().trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return explicit;
    }

    let dirs = dsh_profiles();
    for preferred in ["desktop", "web"] {
        if dirs.iter().any(|dir| dir == preferred) {
            return preferred.to_string();
        }
    }
    match dirs.len() {
        1 => dirs
            .into_iter()
            .next()
            .unwrap_or_else(|| "default".to_string()),
        _ => "default".to_string(),
    }
}

/// Write our relay's models into WorkBuddy's `models.json`.
///
/// # The shape that is actually on disk
///
/// WorkBuddy is an Electron app whose custom models live in
/// `~/.workbuddy/models.json`. Its own writer
/// (`ModelsJsonStorage` in `cli/dist/codebuddy-lite-wb.mjs`) accepts **two**
/// shapes and treats them differently:
///
/// ```text
/// { "models": [ … ], "availableModels": [ "id", … ] }   ← what its UI writes
/// [ … ]                                                  ← what a
///                                                            hand-configured
///                                                            install has
/// ```
///
/// `parseModelsJson` returns `{models: []}` for the array form, while
/// `getModelsJsonApiKeyFields` recognises it (`["*","apiKey"]`). Both are live
/// in the wild, so the writer preserves whichever one it finds and only ever
/// replaces the entries that point at *our* relay.
///
/// # Why one record per model
///
/// A record is `{id, name, vendor, url, apiKey, …}` — the endpoint and key are
/// **per model**, not per provider. The previous writer emitted a single record
/// listing every model under a `models` key that WorkBuddy does not read, so the
/// app showed one unusable entry called `aimyria` and none of the real models.
///
/// # What is preserved
///
/// Entries whose `url` is not ours are left byte-for-byte intact, including
/// their order. Our own entries are recognised by relay host, not by id, so a
/// re-import after a key rotation replaces them instead of accumulating
/// duplicates next to the user's six hand-written models.
pub fn write_workbuddy(settings: &Value) -> Result<(), AppError> {
    write_workbuddy_at(&workbuddy_models_path(), settings)
}

pub fn write_workbuddy_at(path: &Path, settings: &Value) -> Result<(), AppError> {
    let endpoint = settings
        .get("baseUrl")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::InvalidInput("WorkBuddy 配置缺少 baseUrl".to_string()))?;
    let api_key = settings
        .get("apiKey")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let models: Vec<String> = settings
        .get("models")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty() && !is_utility_model(id))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if models.is_empty() {
        return Err(AppError::InvalidInput(
            "WorkBuddy 配置里没有可写入的模型".to_string(),
        ));
    }

    // Which shape is the file? A missing or unreadable file gets the object
    // shape, because that is what WorkBuddy's own settings panel writes and
    // therefore what its reader is best tested against.
    let existing = read_json_file::<Value>(path).ok();
    let as_array = matches!(existing, Some(Value::Array(_)));
    let mut entries: Vec<Value> = match existing {
        Some(Value::Array(items)) => items,
        Some(Value::Object(mut map)) => map
            .remove("models")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    // Drop only the records this import previously wrote; everything else
    // survives in place, including the user's own models on our other hosts.
    entries.retain(|entry| !workbuddy_entry_is_ours(entry, endpoint));

    let fresh: Vec<Value> = models
        .iter()
        .map(|id| workbuddy_model_entry(id, endpoint, api_key))
        .collect();
    let ids: Vec<String> = models.clone();
    entries.splice(0..0, fresh);

    let merged = if as_array {
        Value::Array(entries)
    } else {
        // `availableModels` is the panel's "enabled" list. Keep the user's own
        // picks and make sure ours are visible, without duplicating ids.
        let mut available: Vec<String> = existing_available_models(path);
        for id in &ids {
            if !available.iter().any(|current| current == id) {
                available.push(id.clone());
            }
        }
        json!({ "models": entries, "availableModels": available })
    };

    let text = serde_json::to_string_pretty(&merged)
        .map_err(|e| AppError::InvalidInput(format!("WorkBuddy 配置序列化失败: {e}")))?;
    atomic_write(path, format!("{text}\n").as_bytes())
}

/// One WorkBuddy custom-model record for a relay model id.
///
/// Field names come from `ModelsSettingsPanel` in the installed app: the endpoint
/// is `url`, the capability flags are `supportsToolCall` / `supportsImages` /
/// `supportsReasoning` / `useCustomProtocol`, and the limits are
/// `maxInputTokens` / `maxOutputTokens`. `vendor: "Custom"` is what makes the
/// entry appear under the user's own models rather than a built-in provider.
fn workbuddy_model_entry(id: &str, endpoint: &str, api_key: &str) -> Value {
    let lower = id.to_ascii_lowercase();
    let reasoning = lower.starts_with("deepseek-") || lower.contains("deepseek");
    json!({
        "id": id,
        "name": id,
        "vendor": "Custom",
        "url": endpoint,
        "apiKey": api_key,
        "supportsToolCall": true,
        "supportsImages": false,
        "supportsReasoning": reasoning,
        "useCustomProtocol": false,
        "maxInputTokens": 200_000,
        "maxOutputTokens": 32_000,
    })
}

/// Whether an existing record was written by *this* import.
///
/// Matched on the **exact endpoint** this import is about to write, not on the
/// relay host. The host test was wrong in a way that destroys data: on this
/// machine all six hand-written WorkBuddy models point at `wb.321ai.cc`, which is
/// one of our own domains, so "any of our hosts" classified every one of them as
/// ours and deleted them on the next import.
///
/// Two extra recognisers remain, both narrow:
///
/// - `baseUrl` carrying our host: the field the old writer emitted. It is not a
///   WorkBuddy field, so a record that has it was written by this tool.
/// - the bare `id: "aimyria"` with no url at all: the old writer's single row.
///   A record with an id of `aimyria` but someone else's url is the user's own
///   model and is left alone.
fn workbuddy_entry_is_ours(entry: &Value, endpoint: &str) -> bool {
    let url = entry.get("url").and_then(Value::as_str).map(str::trim);
    if url.is_some_and(|url| url.eq_ignore_ascii_case(endpoint.trim())) {
        return true;
    }
    if let Some(base_url) = entry.get("baseUrl").and_then(Value::as_str) {
        if points_at_our_relay(base_url) {
            return true;
        }
    }
    entry.get("id").and_then(Value::as_str) == Some(PROVIDER_KEY) && url.is_none()
}

/// Put WorkBuddy back on its own built-in models.
///
/// # What "official" means for this client
///
/// WorkBuddy ships a built-in model catalogue. Everything this tool ever added
/// lives in `models.json` as a record with `vendor: "Custom"` and one of our
/// endpoints. So reverting is: **remove our records, keep every other record,
/// and drop our ids from `availableModels`**.
///
/// # What it must not do
///
/// * It must not delete a record just because it says `vendor: "Custom"`. A user
///   who added their own OpenAI-compatible endpoint by hand has one too, and
///   deleting it would destroy their setup. The test is the endpoint, not the
///   vendor.
/// * It must not delete the whole file. `models.json` is the app's file.
/// * It must not fail when there is nothing to remove — "already official" is a
///   success, not an error, or the button looks broken the second time it is
///   pressed.
///
/// # Why it returns what it removed
///
/// So the UI can say "removed 6 entries" instead of silently changing a file the
/// user may be looking at in WorkBuddy's own panel.
pub fn revert_workbuddy_to_official_at(path: &Path) -> Result<usize, AppError> {
    if !path.exists() {
        // Nothing was ever written on this machine.
        return Ok(0);
    }

    let existing = read_json_file::<Value>(path)
        .map_err(|e| AppError::InvalidInput(format!("读取 WorkBuddy 配置失败: {e}")))?;
    let as_array = matches!(existing, Value::Array(_));

    let mut entries: Vec<Value> = match existing {
        Value::Array(items) => items,
        Value::Object(mut map) => map
            .remove("models")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    let before = entries.len();
    let mut removed_ids: Vec<String> = Vec::new();
    entries.retain(|entry| {
        let ours = workbuddy_entry_points_at_our_relay(entry);
        if ours {
            if let Some(id) = entry.get("id").and_then(Value::as_str) {
                removed_ids.push(id.to_string());
            }
        }
        !ours
    });
    let removed = before - entries.len();

    if removed == 0 {
        // Already official. Do not rewrite the file just to prove it.
        return Ok(0);
    }

    let merged = if as_array {
        Value::Array(entries)
    } else {
        // Drop our ids from the panel's enabled list as well; leaving them there
        // makes WorkBuddy show a model that no longer has a record behind it.
        let available: Vec<String> = existing_available_models(path)
            .into_iter()
            .filter(|id| !removed_ids.iter().any(|removed| removed == id))
            .collect();
        json!({ "models": entries, "availableModels": available })
    };

    let text = serde_json::to_string_pretty(&merged)
        .map_err(|e| AppError::InvalidInput(format!("WorkBuddy 配置序列化失败: {e}")))?;
    atomic_write(path, format!("{text}\n").as_bytes())?;

    log::info!(
        "[workbuddy] reverted to official models, removed {removed} entr{}",
        if removed == 1 { "y" } else { "ies" }
    );
    Ok(removed)
}

fn existing_is_array(path: &Path) -> bool {
    matches!(read_json_file::<Value>(path), Ok(Value::Array(_)))
}

/// Was this record written against one of our relays?
///
/// Endpoint-based, deliberately: `vendor: "Custom"` is shared with the user's own
/// third-party models, and an `id`-based test cannot see records the console wrote
/// under a different id.
fn workbuddy_entry_points_at_our_relay(entry: &Value) -> bool {
    let url = entry
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if url.is_some_and(points_at_our_relay) {
        return true;
    }
    // The field the first writer emitted; it is not a WorkBuddy field.
    entry
        .get("baseUrl")
        .and_then(Value::as_str)
        .is_some_and(points_at_our_relay)
}

/// Build a WorkBuddy record for a relay model id.
///
/// The per-model limits come from what the relay actually serves where known:
/// a 1M-context group gets `maxInputTokens: 1_000_000`, and an unknown model
/// keeps conservative defaults rather than claiming a window it does not have.
/// Claiming too much is the failure that shows up as a truncated conversation
/// after the user has already sent the prompt.
fn existing_available_models(path: &Path) -> Vec<String> {
    read_json_file::<Value>(path)
        .ok()
        .and_then(|value| {
            value
                .get("availableModels")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
        })
        .unwrap_or_default()
}

/// Utility models that must not be offered as a chat model in a desktop client.
///
/// The relay's group whitelist can carry embeddings, image models and dated
/// snapshots; writing those into WorkBuddy or dsh gives the user a menu entry
/// that fails the moment it is selected.
fn is_utility_model(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    const NEEDLES: [&str; 9] = [
        "embedding",
        "rerank",
        "whisper",
        "tts",
        "moderation",
        "dall-e",
        "image",
        "video",
        "audio",
    ];
    NEEDLES.iter().any(|needle| lower.contains(needle))
}

/// Whether a host belongs to one of our relays.
/// Does this URL point at one of our relays?
///
/// Delegated to [`crate::relay::hosts`], which owns the host list. The local copy
/// this replaced knew `api.aimyria.com`, `321ai.cc` and a substring match on
/// `aimyria.com` — and therefore **missed `sub2.aimyria.com`**, which is exactly
/// what a newer console import writes. That is the same class of drift that once
/// made the one-click screen report a configured client as unconfigured.
fn points_at_our_relay(url: &str) -> bool {
    crate::relay::is_relay_endpoint(url).is_some()
}

/// Write dsh's `cordis.patch.yml` and credentials.
///
/// # The trap this writer has to avoid
///
/// A dsh patch entry is **id-targeted and replaces that row's whole `config`**:
/// "a patch replaces the targeted row's whole `config`, so each row below
/// restates every key it owns" (`dsh-web-app/cordis.patch.yml` header, quoted in
/// `02-运行系统\调研参考\dsh-technical-report.md` §4.1).
///
/// So emitting our own `- id: llm-pi-ai` with a single `aimyria` provider does
/// not *add* a provider — it **replaces the entire `llm-pi-ai` row**, deleting
/// every other provider the user configured there. A real install carries
/// `aimyria`, `opencode-go` and `a` in that one row. The same applies to
/// `agent-default-model`: writing it would silently repoint the user's default
/// model at us.
///
/// # Why this edits text instead of re-serialising the document
///
/// The obvious implementation — parse with `serde_yaml`, insert, serialise — is
/// wrong here, and the failure is silent. Real profiles contain `!!js`
/// expressions:
///
/// ```text
/// - id: ui
///   disabled: !!js process.platform !== 'win32'
/// ```
///
/// `serde_yaml` parses that tag as a plain string and writes it back without the
/// tag, so the file would still look plausible while dsh lost the conditional.
/// The user's own rows must survive byte-for-byte, so this writer only ever
/// touches the block it owns.
///
/// # What it does
///
/// 1. Remove our marker block, if a previous run left one.
/// 2. If the profile has an `llm-pi-ai` row, insert our provider under its
///    `config.providers` at the existing indentation, leaving the user's other
///    providers untouched.
/// 3. Otherwise append a complete `llm-pi-ai` row carrying only our provider.
/// 4. Add `agent-default-model` **only when the profile has none** — moving the
///    user's default model is their decision, not an import's.
/// 5. Never touch `llm-deepseek`. The old writer disabled it, which broke the
///    user's native DeepSeek account without telling them.
pub fn write_dsh(settings: &Value) -> Result<(), AppError> {
    write_dsh_at(&dsh_profile_patch_path(), &dsh_credentials_path(), settings)
}

pub fn write_dsh_at(
    patch_path: &Path,
    credentials_path: &Path,
    settings: &Value,
) -> Result<(), AppError> {
    let endpoint = settings
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::InvalidInput("DeepSeek Harness 配置缺少 base_url".to_string()))?;
    let api_key = settings
        .get("api_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let models: Vec<String> = settings
        .get("models")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("id").and_then(Value::as_str))
                .map(str::trim)
                .filter(|id| !id.is_empty() && !is_utility_model(id))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if models.is_empty() {
        return Err(AppError::InvalidInput(
            "DeepSeek Harness 配置里没有可写入的模型".to_string(),
        ));
    }
    let primary = settings
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| models[0].clone());

    let existing = std::fs::read_to_string(patch_path).unwrap_or_default();
    let without_ours = remove_marked_block(&existing, DSH_BEGIN, DSH_END);
    let base = if without_ours.trim().is_empty() {
        String::new()
    } else {
        without_ours
    };

    let mut next = match dsh_insert_provider(&base, endpoint, &models) {
        DshInsert::Inserted(text) => text,
        DshInsert::NoRow => {
            let mut text = base.clone();
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&dsh_full_pi_row(endpoint, &models));
            text
        }
    };

    if !dsh_has_row(&next, "agent-default-model") {
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(&format!(
            "{DSH_BEGIN}\n- id: agent-default-model\n  name: \"@deepseek-ai/dsh-agent-default-model\"\n  config:\n    provider: {PROVIDER_KEY}\n    model: {primary}\n{DSH_END}\n"
        ));
    }

    atomic_write(patch_path, next.as_bytes())?;

    write_dsh_credentials(credentials_path, api_key)
}

/// Markers around the default-model row this tool may add.
///
/// The provider block itself carries no marker pair: it lives inside the user's
/// `llm-pi-ai` row, where a marker comment would sit as a sibling of real keys.
/// It is identified by the `aimyria:` provider key, which is also how dsh
/// addresses it.
const DSH_BEGIN: &str = "# aimyria:begin";
const DSH_END: &str = "# aimyria:end";

enum DshInsert {
    Inserted(String),
    NoRow,
}

/// Insert the `aimyria` provider under an existing `llm-pi-ai` row.
///
/// Matched structurally on the text: the row that starts with `- id: llm-pi-ai`,
/// then its `providers:` child, then our block at the indentation the existing
/// entries use. A row whose shape is not recognised counts as "no row" —
/// appending a new one is visible and fixable by the user, while guessing at
/// indentation silently corrupts the file.
fn dsh_insert_provider(source: &str, endpoint: &str, models: &[String]) -> DshInsert {
    let lines: Vec<&str> = source.lines().collect();
    let Some(row_index) = lines
        .iter()
        .position(|line| is_dsh_row_start(line, "llm-pi-ai"))
    else {
        return DshInsert::NoRow;
    };

    let row_end = lines
        .iter()
        .enumerate()
        .skip(row_index + 1)
        .find(|(_, line)| line.starts_with("- "))
        .map(|(index, _)| index)
        .unwrap_or(lines.len());

    let mut providers_index = None;
    for index in row_index + 1..row_end {
        if lines[index].trim() == "providers:" {
            providers_index = Some(index);
            break;
        }
    }
    let Some(providers_index) = providers_index else {
        return DshInsert::NoRow;
    };

    let providers_indent = leading_spaces(lines[providers_index]);
    let child_indent = lines
        .iter()
        .skip(providers_index + 1)
        .take_while(|line| line.trim().is_empty() || leading_spaces(line) > providers_indent)
        .find(|line| !line.trim().is_empty())
        .map(|line| leading_spaces(line))
        .unwrap_or(providers_indent + 2);

    let pad = " ".repeat(child_indent);
    let our_key = format!("{pad}{PROVIDER_KEY}:");

    // Does a provider entry from an earlier import (or a hand-written one) already
    // exist in this row?
    let has_previous = lines[providers_index + 1..row_end]
        .iter()
        .any(|line| *line == our_key);

    // Their tuning is merged, not replaced. `dsh_existing_provider_block` needs
    // the whole row, so it is read from the source before any edit.
    let existing = dsh_existing_provider_block(source, child_indent);
    let block = dsh_provider_text(pad.as_str(), endpoint, models, existing.as_deref());

    if !has_previous {
        // First import: our provider goes first under `providers:`.
        let mut out: Vec<String> = Vec::with_capacity(lines.len() + block.lines().count());
        for (index, line) in lines.iter().enumerate() {
            out.push((*line).to_string());
            if index == providers_index {
                out.extend(block.lines().map(str::to_string));
            }
        }
        let mut text = out.join("\n");
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        return DshInsert::Inserted(text);
    }

    // Re-import: replace the existing entry in place. Appending a second
    // `aimyria:` key to the same mapping produces a duplicate key, which YAML
    // forbids — dsh would read whichever one its parser happened to keep.
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + block.lines().count());
    let mut skipping = false;
    for (index, line) in lines.iter().enumerate() {
        if skipping {
            // Our block ends at the next key at (or above) its own indentation.
            if line.trim().is_empty() || leading_spaces(line) > child_indent {
                continue;
            }
            skipping = false;
        }
        if *line == our_key {
            out.extend(block.lines().map(str::to_string));
            skipping = true;
            continue;
        }
        out.push((*line).to_string());
        let _ = index;
    }

    let mut text = out.join("\n");
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    DshInsert::Inserted(text)
}

fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

fn is_dsh_row_start(line: &str, id: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed == format!("- id: {id}") || trimmed == format!("- id: \"{id}\"")
}

fn dsh_has_row(source: &str, id: &str) -> bool {
    source.lines().any(|line| is_dsh_row_start(line, id))
}

/// The `aimyria` provider block, indented for its position under `providers:`.
///
/// The key arrives through `apiKeyEnv` (resolved from `.credentials.yaml`
/// `refs:`, so it never lands in the patch file), `api` is `openai-completions`,
/// and each model is `{id, name}`.
///
/// `existing` is the user's current block, if any, and it is **merged** rather
/// than replaced. Only the three fields this tool is responsible for are
/// rewritten — `baseURL` (the point of the import), `models` (the group's current
/// whitelist) and `apiKeyEnv` (fixed, because that is the name the key is stored
/// under). Everything else survives: `reasoning`, `compat`,
/// `defaultContextWindow`, `defaultMaxTokens`, and any field a newer dsh adds
/// that this build has never heard of.
///
/// The first version replaced the whole block. On this machine the user's own
/// block carried `reasoning: high` and a `compat:` section tuned for DeepSeek
/// reasoning models; replacing it would have degraded every request while the
/// configuration still looked correct.
fn dsh_provider_text(pad: &str, endpoint: &str, models: &[String], existing: Option<&str>) -> String {
    // Fields this tool owns. A key at this indentation is ours to rewrite; the
    // list that follows `models:` disappears with it.
    const OURS: [&str; 3] = ["baseURL", "models", "apiKeyEnv"];

    let mut kept: Vec<&str> = Vec::new();
    if let Some(existing) = existing {
        let child_indent = pad.len() + 2;
        let mut skipping = false;
        for line in existing.lines().skip(1) {
            if line.trim().is_empty() {
                continue;
            }
            let indent = leading_spaces(line);
            if indent <= pad.len() {
                break;
            }
            if indent == child_indent {
                let key = line.trim_start().split(':').next().unwrap_or("").trim();
                skipping = OURS.contains(&key);
            }
            if !skipping {
                kept.push(line);
            }
        }
    }

    let mut text = format!("{pad}{PROVIDER_KEY}:\n");
    if !kept.iter().any(|line| line.trim_start().starts_with("displayName:")) {
        text.push_str(&format!("{pad}  displayName: Aimyria\n"));
    }
    for line in kept {
        text.push_str(line);
        text.push('\n');
    }
    text.push_str(&format!("{pad}  apiKeyEnv: AIMYRIA_API_KEY\n"));
    text.push_str(&format!("{pad}  baseURL: {endpoint}\n"));
    text.push_str(&format!("{pad}  models:\n"));
    for id in models {
        text.push_str(&format!("{pad}    - id: {id}\n{pad}      name: {id}\n"));
    }
    text
}

/// The text of an existing `aimyria:` block — key line first, no trailing
/// siblings. `None` when the profile has no such block.
fn dsh_existing_provider_block(source: &str, child_indent: usize) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    let key = format!("{}{PROVIDER_KEY}:", " ".repeat(child_indent));
    let start = lines.iter().position(|line| *line == key)?;
    let mut end = lines.len();
    for (index, line) in lines.iter().enumerate().skip(start + 1) {
        if line.trim().is_empty() {
            continue;
        }
        if leading_spaces(line) <= child_indent {
            end = index;
            break;
        }
    }
    Some(lines[start..end].join("\n"))
}

/// A complete `llm-pi-ai` row carrying only our provider, for a profile that has
/// none. Top-level indentation, matching the file's other rows.
fn dsh_full_pi_row(endpoint: &str, models: &[String]) -> String {
    format!(
        "- id: llm-pi-ai\n  name: \"@deepseek-ai/dsh-llm-pi-ai\"\n  config:\n    providers:\n{}",
        dsh_provider_text("      ", endpoint, models, None)
    )
}

/// Strip a marker-delimited block, keeping every other byte.
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
///
/// This is the *transport* shape: the endpoint and key, plus the model ids as a
/// flat list. `write_workbuddy_at` turns it into one record per model, because
/// that is what WorkBuddy's `models.json` holds — see that function for why a
/// single record cannot carry a list.
/// The endpoint and key a file-based client is currently using.
///
/// # Why the two clients need their own reader
///
/// WorkBuddy and DeepSeek Harness have no provider row — their key lives in their
/// own config file — so `Provider::resolve_usage_credentials` cannot answer for
/// them. The keys screen needs the same answer for every client, which means this
/// one function has to speak both file formats.
///
/// Returns `(endpoint, secret)`. Either half may be empty when the file exists but
/// does not carry that field; the caller decides whether that counts as bound.
pub fn read_live_credentials(app: AimyriaOnlyApp) -> Option<(String, String)> {
    match app {
        AimyriaOnlyApp::WorkBuddy => {
            let text = std::fs::read_to_string(app.live_path()).ok()?;
            let json: Value = serde_json::from_str(&text).ok()?;
            // The file is either a bare array or an object with `models`.
            let entries = json
                .as_array()
                .cloned()
                .or_else(|| json.get("models").and_then(|v| v.as_array()).cloned())?;
            for entry in entries {
                let url = entry.get("url").and_then(Value::as_str).unwrap_or("");
                let key = entry.get("apiKey").and_then(Value::as_str).unwrap_or("");
                if !key.trim().is_empty() {
                    return Some((url.trim().to_string(), key.trim().to_string()));
                }
            }
            None
        }
        AimyriaOnlyApp::DeepSeekHarness => {
            // Endpoint lives in the profile patch, the key in the credentials file
            // beside it. Both are needed for a useful answer, but a missing endpoint
            // still tells us which key is bound, so fall back to an empty endpoint.
            let patch = std::fs::read_to_string(app.live_path()).unwrap_or_default();
            let endpoint = patch
                .lines()
                .find_map(|line| {
                    let trimmed = line.trim();
                    trimmed.strip_prefix("baseURL:").map(|rest| {
                        rest.trim().trim_matches('"').trim().to_string()
                    })
                })
                .unwrap_or_default();
            let key = dsh_credential_secret()?;
            Some((endpoint, key))
        }
    }
}

/// The `AIMYRIA_API_KEY` value from whichever dsh profile holds it.
fn dsh_credential_secret() -> Option<String> {
    // The import writes the key next to the profile it configured, and this machine
    // has more than one profile (`desktop`, `web`), so scan rather than guess.
    for name in dsh_profiles() {
        let path = dsh_home()
            .join("profiles")
            .join(&name)
            .join(".credentials.yaml");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in text.lines() {
            if let Some(rest) = line.trim().strip_prefix("AIMYRIA_API_KEY:") {
                let value = rest.trim().trim_matches('"').trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

pub fn build_workbuddy_settings(endpoint: &str, api_key: &str, models: &[String]) -> Value {
    json!({
        "provider": PROVIDER_KEY,
        "baseUrl": endpoint,
        "apiKey": api_key,
        "model": models.first().cloned().unwrap_or_default(),
        "models": models,
    })
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

    const RELAY: &str = "https://api.aimyria.com/v1";

    fn workbuddy_settings(models: &[&str]) -> Value {
        json!({
            "provider": PROVIDER_KEY,
            "baseUrl": RELAY,
            "apiKey": "sk-test",
            "model": models.first().copied().unwrap_or_default(),
            "models": models,
        })
    }

    /// The shape WorkBuddy actually writes: one record per custom model, with
    /// the endpoint and key on each record. A single record holding a `models`
    /// array — what this writer used to emit — lists in the app as one model
    /// called `aimyria` that cannot be called.
    #[test]
    fn workbuddy_writes_one_record_per_model_with_the_real_field_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");

        write_workbuddy_at(
            &path,
            &workbuddy_settings(&["deepseek-v4.1-flash", "glm-5.3"]),
        )
        .unwrap();

        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let models = value["models"].as_array().unwrap();
        assert_eq!(models.len(), 2);
        for entry in models {
            assert_eq!(entry["vendor"], "Custom");
            assert_eq!(entry["url"], RELAY);
            assert_eq!(entry["apiKey"], "sk-test");
            assert_eq!(entry["supportsToolCall"], true);
            assert_eq!(entry["useCustomProtocol"], false);
            // The names WorkBuddy reads, not the ones this crate used to invent.
            assert!(
                entry.get("baseUrl").is_none(),
                "baseUrl is not a WorkBuddy field"
            );
            assert!(entry.get("supportsToolsCall").is_none());
            assert!(entry.get("supportsCustomProtocol").is_none());
        }
        assert_eq!(models[0]["id"], "deepseek-v4.1-flash");
        assert_eq!(models[1]["id"], "glm-5.3");
    }

    /// A hand-configured install stores a bare array. Reading it as an object
    /// returned an error, so the import failed — and had the branch succeeded,
    /// it would have replaced the array with an object and lost every model.
    #[test]
    fn workbuddy_reads_and_preserves_the_bare_array_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        std::fs::write(
            &path,
            r#"[{"id":"cn:deepseek-v4.1-flash","name":"mine","vendor":"Custom","url":"https://wb.321ai.cc/v1","apiKey":"other"},
                {"id":"global:gpt-6.1-sol","name":"mine too","vendor":"Custom","url":"https://wb.321ai.cc/v1","apiKey":"other"}]"#,
        )
        .unwrap();

        write_workbuddy_at(&path, &workbuddy_settings(&["glm-5.3"])).unwrap();

        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let models = value.as_array().expect("an array input must stay an array");
        assert_eq!(models.len(), 3, "two user models + one ours");
        assert_eq!(models[1]["id"], "cn:deepseek-v4.1-flash");
        assert_eq!(models[2]["id"], "global:gpt-6.1-sol");
        assert_eq!(models[0]["url"], RELAY);
    }

    /// Re-importing after a key rotation replaces our records instead of piling
    /// duplicates next to the user's own models.
    #[test]
    fn workbuddy_reimport_replaces_our_records_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        std::fs::write(
            &path,
            r#"{"models":[{"id":"hand-written","url":"https://elsewhere.example/v1"}],
                "availableModels":["hand-written"]}"#,
        )
        .unwrap();

        write_workbuddy_at(&path, &workbuddy_settings(&["glm-5.3"])).unwrap();
        write_workbuddy_at(&path, &workbuddy_settings(&["glm-5.3"])).unwrap();

        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let models = value["models"].as_array().unwrap();
        assert_eq!(models.len(), 2, "one of ours + one of theirs");
        assert_eq!(models[1]["id"], "hand-written");
        let available = value["availableModels"].as_array().unwrap();
        assert!(available.iter().any(|id| id == "hand-written"));
        assert!(available.iter().any(|id| id == "glm-5.3"));
    }

    /// The user's own models can live on **our** other hosts — this machine's
    /// six hand-written entries all point at `wb.321ai.cc`. Matching "any of our
    /// hosts" therefore treated all six as ours and deleted them on import.
    ///
    /// Identity has to be the exact endpoint this import writes.
    #[test]
    fn workbuddy_keeps_user_models_that_happen_to_use_our_other_hosts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        let user_row = json!({
            "id": "cn:deepseek-v4.1-flash",
            "name": "mine",
            "vendor": "Custom",
            "url": "https://wb.321ai.cc/v1",
            "apiKey": "user-key",
        });
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&json!({ "models": [user_row] })).unwrap(),
        )
        .unwrap();

        // Import against a *different* one of our endpoints.
        write_workbuddy_at(&path, &workbuddy_settings(&["glm-5.3"])).unwrap();

        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let models = value["models"].as_array().unwrap();
        assert_eq!(
            models.len(),
            2,
            "the user's model on our other host must survive: {models:?}"
        );
        assert!(
            models.iter().any(|m| m["url"] == "https://wb.321ai.cc/v1"),
            "the wb.321ai.cc row must still be there"
        );
        assert!(models.iter().any(|m| m["url"] == RELAY));
    }

    /// The old writer also carried the legacy single-entry id; a stale row from
    /// that version has no relay url we can match, so it is recognised by id.
    #[test]
    fn workbuddy_removes_the_legacy_single_entry_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        std::fs::write(
            &path,
            r#"{"models":[{"id":"aimyria","name":"Aimyria","baseUrl":"https://api.aimyria.com"}]}"#,
        )
        .unwrap();

        write_workbuddy_at(&path, &workbuddy_settings(&["glm-5.3"])).unwrap();

        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let models = value["models"].as_array().unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["id"], "glm-5.3");
    }

    /// Utility models in a relay whitelist must not become chat menu entries.
    #[test]
    fn workbuddy_skips_utility_models() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        write_workbuddy_at(
            &path,
            &workbuddy_settings(&["glm-5.3", "text-embedding-3-large", "gpt-image-1"]),
        )
        .unwrap();
        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let models = value["models"].as_array().unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["id"], "glm-5.3");
    }

    fn dsh_settings(models: &[&str]) -> Value {
        json!({
            "base_url": RELAY,
            "api_key": "sk-test",
            "api_mode": "chat_completions",
            "model": models.first().copied().unwrap_or_default(),
            "models": models.iter().map(|id| json!({"id": id, "name": id})).collect::<Vec<_>>(),
        })
    }

    /// The case that matters: a real profile already has `llm-pi-ai` carrying
    /// the user's own providers. dsh replaces a targeted row's whole `config`,
    /// so appending a second `llm-pi-ai` row would delete those providers.
    #[test]
    fn dsh_merges_into_the_existing_pi_ai_row_and_keeps_other_providers() {
        let dir = tempfile::tempdir().unwrap();
        let patch = dir.path().join("cordis.patch.yml");
        let credentials = dir.path().join(".credentials.yaml");
        std::fs::write(
            &patch,
            r#"- id: llm-pi-ai
  name: "@deepseek-ai/dsh-llm-pi-ai"
  config:
    providers:
      opencode-go:
        apiKeyEnv: OPENCODE_GO_API_KEY
      a:
        displayName: Aimyria-wb
        baseURL: https://wb.321ai.cc/v1
- id: agent-default-model
  config:
    provider: a
    model: cn:deepseek-v4.1-flash
"#,
        )
        .unwrap();

        write_dsh_at(
            &patch,
            &credentials,
            &dsh_settings(&["glm-5.3", "deepseek-v4.1-flash"]),
        )
        .unwrap();

        let text = std::fs::read_to_string(&patch).unwrap();
        assert_eq!(
            text.matches("- id: llm-pi-ai").count(),
            1,
            "exactly one llm-pi-ai row, or dsh has two competing patches"
        );
        assert!(
            text.contains("opencode-go"),
            "the user's provider must survive: {text}"
        );
        assert!(
            text.contains("Aimyria-wb"),
            "the user's provider must survive: {text}"
        );
        assert!(text.contains("glm-5.3"));
        // Our provider sits under the existing row, at the same depth as theirs.
        assert!(
            text.contains("      aimyria:\n"),
            "provider inserted at the existing indentation: {text}"
        );
        assert!(
            text.contains("      opencode-go:"),
            "the insertion must not re-indent their sibling: {text}"
        );

        // The user already had a default model; moving it is not ours to do.
        assert!(
            text.contains("provider: a"),
            "their default model must stay theirs"
        );
        assert!(
            !text.contains("provider: aimyria"),
            "we must not repoint their default"
        );
        // And the native DeepSeek account must not be disabled.
        assert!(
            !text.contains("llm-deepseek"),
            "we must not disable their native provider"
        );

        let creds = std::fs::read_to_string(&credentials).unwrap();
        assert!(creds.contains("AIMYRIA_API_KEY: sk-test"));
    }

    /// A real dsh profile contains `!!js` expressions. Parsing and re-serialising
    /// with `serde_yaml` turns `!!js process.platform !== 'win32'` into a plain
    /// string, which silently disables the user's own conditional while leaving
    /// a file that still looks right.
    #[test]
    fn dsh_preserves_js_tags_and_every_other_byte_of_the_users_rows() {
        let dir = tempfile::tempdir().unwrap();
        let patch = dir.path().join("cordis.patch.yml");
        let credentials = dir.path().join(".credentials.yaml");
        let original = concat!(
            "- id: ui\n",
            "  disabled: !!js process.platform !== 'win32'\n",
            "- id: llm-pi-ai\n",
            "  config:\n",
            "    providers:\n",
            "      a:\n",
            "        baseURL: https://wb.321ai.cc/v1\n",
        );
        std::fs::write(&patch, original).unwrap();

        write_dsh_at(&patch, &credentials, &dsh_settings(&["glm-5.3"])).unwrap();

        let text = std::fs::read_to_string(&patch).unwrap();
        assert!(
            text.contains("disabled: !!js process.platform !== 'win32'"),
            "the !!js tag must survive verbatim: {text}"
        );
        assert!(
            text.contains("        baseURL: https://wb.321ai.cc/v1"),
            "untouched rows keep their exact bytes: {text}"
        );
    }

    #[test]
    fn dsh_rewrite_is_idempotent_and_preserves_other_patches() {
        let dir = tempfile::tempdir().unwrap();
        let patch = dir.path().join("cordis.patch.yml");
        let credentials = dir.path().join(".credentials.yaml");
        std::fs::write(&patch, "- id: user-plugin\n  config:\n    keep: true\n").unwrap();
        let settings = dsh_settings(&["deepseek-v4.1-flash", "glm-5.3"]);

        write_dsh_at(&patch, &credentials, &settings).unwrap();
        let first = std::fs::read_to_string(&patch).unwrap();
        write_dsh_at(&patch, &credentials, &settings).unwrap();
        let second = std::fs::read_to_string(&patch).unwrap();

        assert_eq!(first, second, "a second identical import changes nothing");
        assert!(second.contains("user-plugin"));
        assert_eq!(
            second.matches("- id: llm-pi-ai").count(),
            1,
            "exactly one llm-pi-ai row after two imports"
        );
        assert_eq!(
            second.matches("      aimyria:").count(),
            1,
            "our provider appears once, not once per import: {second}"
        );
        assert_eq!(
            second.matches("glm-5.3").count(),
            2,
            "one id + one name, no duplicates"
        );
    }

    /// A minimal profile has no `llm-pi-ai` row at all. Ours is added, and a
    /// default model is seeded because dsh cannot boot without one.
    #[test]
    fn dsh_creates_the_row_and_a_default_model_when_the_profile_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let patch = dir.path().join("cordis.patch.yml");
        let credentials = dir.path().join(".credentials.yaml");

        write_dsh_at(&patch, &credentials, &dsh_settings(&["glm-5.3"])).unwrap();

        let text = std::fs::read_to_string(&patch).unwrap();
        assert!(text.contains("llm-pi-ai"));
        assert!(text.contains("agent-default-model"));
        assert!(text.contains("provider: aimyria"));
        // The native provider is not in this profile, so there is nothing to
        // disable — and nothing must be invented.
        assert!(!text.contains("llm-deepseek"));
    }

    /// `node_modules` sits beside the profiles and is not one. Counting it made
    /// the old "exactly one profile" test fail on every real install, which sent
    /// the write to a `default` profile dsh never boots.
    #[test]
    fn dsh_ignores_node_modules_when_listing_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let profiles = dir.path().join("profiles");
        std::fs::create_dir_all(profiles.join("desktop")).unwrap();
        std::fs::create_dir_all(profiles.join("web")).unwrap();
        std::fs::create_dir_all(profiles.join("node_modules")).unwrap();

        // `dsh_home()` reads the process environment, so this test pins the
        // rule rather than the function: node_modules must never be a candidate.
        let listed: Vec<String> = std::fs::read_dir(&profiles)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name != "node_modules" && !name.starts_with('.'))
            .collect();
        assert_eq!(listed.len(), 2);
        assert!(!listed.contains(&"node_modules".to_string()));
    }

    #[test]
    fn dsh_prefers_the_desktop_profile() {
        let dir = tempfile::tempdir().unwrap();
        let profiles = dir.path().join("profiles");
        std::fs::create_dir_all(profiles.join("desktop")).unwrap();
        std::fs::create_dir_all(profiles.join("web")).unwrap();
        // Mirrors `dsh_profile_name`'s preference order over the same listing.
        let listed: Vec<String> = std::fs::read_dir(&profiles)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        let chosen = ["desktop", "web"]
            .into_iter()
            .find(|name| listed.iter().any(|dir| dir == name));
        assert_eq!(chosen, Some("desktop"));
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

    // ── revert to official ─────────────────────────────────────────────────

    /// Reverting withdraws our records and keeps everything else.
    ///
    /// The data-loss risk here is real: this machine's own `models.json` had six
    /// hand-written records, five of them on `wb.321ai.cc` — one of our domains.
    /// Any rule that matches "vendor Custom" or "our host" rather than the specific
    /// endpoint would delete the user's models along with ours.
    #[test]
    fn reverting_removes_only_records_aimed_at_our_relays() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");

        let file = json!([
            { "id": "ours-1", "vendor": "Custom", "url": "https://api.aimyria.com/v1" },
            { "id": "ours-2", "vendor": "Custom", "url": "https://sub2.aimyria.com/v1" },
            { "id": "theirs", "vendor": "Custom", "url": "https://my-own-endpoint.example/v1" },
            { "id": "builtin", "vendor": "DeepSeek", "url": "https://api.deepseek.com/v1" }
        ]);
        std::fs::write(&path, file.to_string()).unwrap();

        let removed = revert_workbuddy_to_official_at(&path).expect("revert");
        assert_eq!(removed, 2, "only our two records are withdrawn");

        let after: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let ids: Vec<&str> = after
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| entry.get("id").and_then(Value::as_str))
            .collect();
        assert_eq!(ids, vec!["theirs", "builtin"]);
    }

    /// Reverting when nothing of ours is present succeeds with a count of zero.
    ///
    /// A button that errors the second time it is pressed looks broken, and the
    /// second press is exactly what a user does when they are unsure it worked.
    #[test]
    fn reverting_an_already_official_file_is_a_no_op_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        std::fs::write(
            &path,
            json!([{ "id": "builtin", "vendor": "DeepSeek", "url": "https://api.deepseek.com/v1" }])
                .to_string(),
        )
        .unwrap();
        let before = std::fs::read_to_string(&path).unwrap();

        let removed = revert_workbuddy_to_official_at(&path).expect("must not error");
        assert_eq!(removed, 0);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            before,
            "a no-op must not rewrite the file"
        );
    }

    /// A missing file is "never configured", not an error.
    #[test]
    fn reverting_a_missing_file_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.json");
        assert_eq!(revert_workbuddy_to_official_at(&path).unwrap(), 0);
        assert!(!path.exists(), "no file should be created");
    }

    /// The object shape keeps `availableModels` in sync, or WorkBuddy keeps listing
    /// a model that no longer has a record behind it.
    #[test]
    fn reverting_the_object_shape_also_prunes_the_enabled_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        std::fs::write(
            &path,
            json!({
                "models": [
                    { "id": "ours", "vendor": "Custom", "url": "https://api.aimyria.com/v1" },
                    { "id": "theirs", "vendor": "Custom", "url": "https://mine.example/v1" }
                ],
                "availableModels": ["ours", "theirs"]
            })
            .to_string(),
        )
        .unwrap();

        assert_eq!(revert_workbuddy_to_official_at(&path).unwrap(), 1);

        let after: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let available: Vec<&str> = after["availableModels"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(available, vec!["theirs"], "our id must leave the enabled list");
    }

    /// `sub2.aimyria.com` counts as ours.
    ///
    /// The predicate this replaced listed `api.aimyria.com`, `321ai.cc`, and a
    /// substring test for `aimyria.com` — which happens to miss nothing on its own,
    /// but the point is the list is now the shared one, so a host added to
    /// `relay/hosts.rs` is recognised here automatically.
    #[test]
    fn the_sub_gateway_is_recognised_as_ours() {
        assert!(points_at_our_relay("https://sub2.aimyria.com/v1"));
        assert!(points_at_our_relay("https://api.aimyria.com/v1"));
        assert!(points_at_our_relay("https://aimyria.com/v1"));
        assert!(!points_at_our_relay("https://notaimyria.com/v1"));
        assert!(!points_at_our_relay("https://aimyria.com.evil.example"));
    }
}
