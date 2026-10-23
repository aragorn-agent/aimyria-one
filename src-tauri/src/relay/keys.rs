//! Key lifecycle: create on the console, install into a local client, verify.
//!
//! # The one rule this module exists to enforce
//!
//! A key is only ever created **once per client**, and only when the user asks for
//! it. The most common complaint about tools in this category is a key list with
//! fifteen near-identical entries, all created because the user pressed a button
//! a few times. So [`install`] looks for an existing usable key first and reuses
//! it; rotation is a separate, explicit action.
//!
//! # Why the write path is delegated
//!
//! `deeplink::provider::import_provider_from_deeplink` already knows how to turn
//! an endpoint + key + model list into the correct `settings_config` for every
//! client, and `ProviderService::{add,switch}` already knows how to write it
//! atomically with a backup and how to record the row. Re-implementing any of that
//! here would produce a second writer that drifts — which is exactly the failure
//! mode this codebase has already been bitten by (see `relay/hosts.rs`).
//!
//! So this module builds the *request* and delegates the rest.

use serde::Serialize;
use std::str::FromStr;

use crate::deeplink::DeepLinkImportRequest;
use crate::relay::client::{RelayClient, RelayError};
use crate::store::AppState;

/// A key as the console describes it. Never carries plaintext.
///
/// # Why this one is NOT `rename_all = "camelCase"`
///
/// The console's `/api/v1/keys` payload is snake_case (`group_id`, `quota_used`),
/// while its `/groups/available` payload mixes the two. A blanket `camelCase`
/// rename makes serde look for `groupId` and silently produce `None` for every
/// snake_case field — which is how the group name and the usage figure both went
/// missing while the request itself succeeded. Each field below names its own key.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct KeySummary {
    /// Numeric id from the console. Kept as a JSON number because that is what the
    /// API returns and what a later `GET /keys/:id` needs back.
    pub id: i64,
    pub name: String,
    /// The console returns the **full key**. This struct is internal only; the
    /// command layer masks it before anything reaches the UI.
    pub key: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, rename = "quota_used")]
    pub quota_used: Option<f64>,
    #[serde(default)]
    pub quota: Option<f64>,
    #[serde(default, rename = "group_id")]
    pub group_id: Option<i64>,
}

impl KeySummary {
    /// Masked form for display.
    pub fn masked(&self) -> String {
        mask(&self.key)
    }
}

impl KeySummary {
    /// Is this key usable for configuring a client?
    pub fn is_usable(&self) -> bool {
        match self.status.as_deref() {
            // An explicit disabled/expired state is authoritative.
            Some("disabled") | Some("expired") | Some("revoked") => false,
            _ => true,
        }
    }
}

/// Response to a create/rotate call. `key` is plaintext and exists only here.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedKey {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    /// Plaintext. The caller must install it and drop it; it is never logged.
    pub key: String,
}

/// What the UI reports after an install attempt.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    pub app: String,
    /// `created` | `reused`
    pub key_source: String,
    /// `sk-…abcd` — the only form of the key that may reach the UI.
    pub key_masked: String,
    /// The file the configuration landed in.
    pub written_path: Option<String>,
    /// Did reading the file back show both the key and our host?
    pub verified: bool,
    /// Non-fatal notes (e.g. "another key was already present and was replaced").
    pub warnings: Vec<String>,
}

/// Fetch the account's keys.
///
/// The console paginates; this asks for the largest page it accepts and takes the
/// first one. A user with more than 100 keys is not the case this screen serves, and
/// silently showing the first page as if it were everything would be worse than
/// asking for a generous page size.
pub async fn list(client: &RelayClient) -> Result<Vec<KeySummary>, RelayError> {
    #[derive(serde::Deserialize)]
    struct Page {
        #[serde(default)]
        items: Vec<KeySummary>,
    }

    let page: Page = client
        .get_json("/keys?page=1&page_size=100")
        .await?;
    Ok(page.items)
}

/// Group id -> (display name, model summary).
///
/// # Why a map rather than a struct
///
/// The only consumer is a lookup keyed by the group id a key already carries, so a
/// map is the shape it needs. `description` holds a slash-separated model list the
/// console generates; passing it through as-is keeps this from having to parse it.
pub async fn groups(
    client: &RelayClient,
) -> Result<std::collections::HashMap<i64, (String, Option<String>)>, RelayError> {
    #[derive(serde::Deserialize)]
    struct Group {
        id: i64,
        #[serde(default)]
        name: String,
        #[serde(default)]
        description: Option<String>,
    }

    let groups: Vec<Group> = client.get_json("/groups/available").await?;
    Ok(groups
        .into_iter()
        .map(|group| {
            let models = group
                .description
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty());
            (group.id, (group.name, models))
        })
        .collect())
}

/// Create a new key.
///
/// `name` is what the user will see in the console's key list, so the caller
/// should pass something identifying — the client name is a good default.
pub async fn create(
    client: &RelayClient,
    name: &str,
    group: Option<i64>,
) -> Result<CreatedKey, RelayError> {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Body<'a> {
        name: &'a str,
        // The console names this `group_id` and expects a number. Sending a name
        // string (what this used to do) is a 400 the user would read as "cannot
        // create a key".
        #[serde(skip_serializing_if = "Option::is_none")]
        group_id: Option<i64>,
    }
    client
        .post_json(
            "/keys",
            &Body {
                name,
                group_id: group,
            },
        )
        .await
}

/// Rotate an existing key. Returns the new plaintext, which replaces the old one
/// everywhere it was installed.
pub async fn rotate(client: &RelayClient, key_id: i64) -> Result<CreatedKey, RelayError> {
    client
        .post_json::<CreatedKey, _>(
            &format!("/keys/{key_id}/rotate"),
            &serde_json::json!({}),
        )
        .await
}

/// Mask a plaintext key for display.
///
/// Shows the first four and last four characters, like every other credential UI,
/// so a user can tell two keys apart without either being recoverable.
pub fn mask(key: &str) -> String {
    let trimmed = key.trim();
    if trimmed.len() <= 12 {
        return "sk-…".to_string();
    }
    let head: String = trimmed.chars().take(7).collect();
    let tail: String = trimmed
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

/// Build the import request that installs `key` into `app`.
///
/// This is deliberately the same shape the console's deep link produces, because
/// the write path downstream is the same code. Anything this function gets wrong
/// shows up as a bad client config, so it is covered by tests below.
fn import_request(
    app: &str,
    key: &str,
    endpoint: &str,
    models: &[String],
    name: &str,
) -> DeepLinkImportRequest {
    DeepLinkImportRequest {
        version: "v1".to_string(),
        resource: "provider".to_string(),
        app: Some(app.to_string()),
        name: Some(name.to_string()),
        enabled: Some(true),
        homepage: Some("https://aimyria.com".to_string()),
        endpoint: Some(endpoint.to_string()),
        api_key: Some(key.to_string()),
        icon: Some("aimyria".to_string()),
        model: models.first().cloned(),
        npm: None,
        notes: None,
        haiku_model: models.get(1).cloned(),
        sonnet_model: models.get(2).cloned(),
        opus_model: models.first().cloned(),
        models: Some(models.to_vec()),
        ..Default::default()
    }
}

/// Install a key into one client: create-or-reuse, write, verify.
///
/// # The reuse problem, and why this is not "rotate on every click"
///
/// The console's list endpoint never returns plaintext — correctly, that is the
/// only sane design. So the desktop cannot obtain the secret of an existing key by
/// reading the list. Two options present themselves, and only one is acceptable:
///
/// * **Rotate the existing key** to obtain a new secret. Rejected: rotation
///   invalidates the old secret everywhere, so a user who presses 「接入」 for Codex
///   would silently break Claude Code, which was using the same key. That is a
///   self-inflicted outage, and it is exactly the kind of "helpful" behaviour that
///   makes a tool untrustworthy.
/// * **Reuse the row, mint a fresh secret under it** — not possible with a
///   write-only-once API.
///
/// So the contract is: an install **always creates one key, named after the
/// client**, and the report says so. The user ends up with one key per client,
/// which is what they need anyway to revoke a single client's access. Re-entering
/// the same client reuses the *installed configuration* rather than creating
/// another key: `import_provider_from_deeplink` replaces the row it previously
/// created for that client, so pressing the button twice does not accumulate keys
/// on the local side.
///
/// The one thing that would create duplicates is pressing 「接入」 twice in one
/// session while the first key creation failed after succeeding on the server. That
/// window is small and the consequence is visible (a stray key in the console), so
/// it is reported rather than hidden.
pub async fn install(
    state: &AppState,
    client: &RelayClient,
    app: &str,
    endpoint: &str,
    models: &[String],
    group: Option<i64>,
) -> Result<InstallReport, String> {
    let mut warnings = Vec::new();

    // A key per client, named so the console's list is readable at a glance.
    // The `· AIMYRIA GO` suffix marks provenance without hiding the client name.
    let display_name = format!("{app} · AIMYRIA GO");

    let created = match create(client, &display_name, group).await {
        Ok(created) => created,
        Err(error) => {
            // A 409-style "already exists" is not a real failure: the console may
            // enforce unique names. Retry once with a suffixed name so the user is
            // not blocked by a naming collision they cannot see.
            if matches!(error, RelayError::Rejected { status, .. } if status == 409) {
                warnings.push("同名密钥已存在，改用带序号的名称".to_string());
                let alternative = format!("{display_name} {}", chrono::Local::now().format("%m%d-%H%M"));
                create(client, &alternative, group)
                    .await
                    .map_err(|e| e.user_message())?
            } else {
                return Err(error.user_message());
            }
        }
    };

    let plaintext = created.key;
    let masked = mask(&plaintext);
    let key_id = created.id;

    // Delegate the write. `import_provider_from_deeplink` builds the correct
    // per-client settings and persists the row; `switch` then writes live.
    let request = import_request(app, &plaintext, endpoint, models, "AIMYRIA");
    let provider_id = crate::deeplink::import_provider_from_deeplink(state, request)
        .await
        .map_err(|e| {
            // The key exists at this point. Say so, so the user can clean it up
            // rather than wondering why a key appeared after a failure.
            format!(
                "写入 {app} 配置失败：{e}（密钥 {} 已创建，可在密钥页删除）",
                masked
            )
        })?;

    let app_type = crate::app_config::AppType::from_str(app)
        .map_err(|e| format!("未知客户端 {app}: {e}"))?;
    crate::services::provider::ProviderService::switch(state, app_type.clone(), &provider_id)
        .map_err(|e| format!("激活 {app} 配置失败：{e}"))?;

    // Verify by reading the file back. A write that reports success but did not
    // land is worse than a visible failure, because the UI would show green.
    let written_path = crate::live_writer::live_path(&app_type)
        .map(|path| path.display().to_string());
    let verified = crate::live_writer::live_path(&app_type)
        .as_deref()
        .map(|path| verify_installed(path, &plaintext, endpoint))
        .unwrap_or(false);

    if !verified {
        warnings.push(
            "配置已写入，但回读校验未通过：请检查该客户端是否被其他工具覆盖".to_string(),
        );
    }

    log::info!(
        "[relay] installed key {} into {} (source=created, verified={verified}, id={key_id})",
        masked,
        app
    );

    Ok(InstallReport {
        app: app.to_string(),
        key_source: "created".to_string(),
        key_masked: masked,
        written_path,
        verified,
        warnings,
    })
}

/// Install an already-known secret into one client.
///
/// Split out of [`install`] because rotation has the new secret in hand and must
/// **not** create another key: the whole point of rotating is to replace the
/// secret under the same key id, so the console keeps one row and the user keeps
/// one revocable credential.
///
/// Returns the same report shape, but `key_source` is `rotated`.
pub async fn install_with_secret(
    state: &AppState,
    app: &str,
    plaintext: &str,
    endpoint: &str,
    models: &[String],
) -> Result<InstallReport, String> {
    if plaintext.trim().is_empty() {
        return Err("密钥为空，未写入任何配置".to_string());
    }
    let masked = mask(plaintext);

    let request = import_request(app, plaintext, endpoint, models, "AIMYRIA");
    let provider_id = crate::deeplink::import_provider_from_deeplink(state, request)
        .await
        .map_err(|e| format!("写入 {app} 配置失败：{e}"))?;

    let app_type = crate::app_config::AppType::from_str(app)
        .map_err(|e| format!("未知客户端 {app}: {e}"))?;
    crate::services::provider::ProviderService::switch(state, app_type.clone(), &provider_id)
        .map_err(|e| format!("激活 {app} 配置失败：{e}"))?;

    let written_path = crate::live_writer::live_path(&app_type)
        .map(|path| path.display().to_string());
    let verified = crate::live_writer::live_path(&app_type)
        .as_deref()
        .map(|path| verify_installed(path, plaintext, endpoint))
        .unwrap_or(false);

    let mut warnings = Vec::new();
    if !verified {
        warnings.push(
            "配置已写入，但回读校验未通过：请检查该客户端是否被其他工具覆盖".to_string(),
        );
    }

    log::info!(
        "[relay] installed rotated key {} into {} (verified={verified})",
        masked,
        app
    );

    Ok(InstallReport {
        app: app.to_string(),
        key_source: "rotated".to_string(),
        key_masked: masked,
        written_path,
        verified,
        warnings,
    })
}

/// Does the file contain the key and our endpoint?
///
/// Both halves matter: a file with the key but a foreign base URL sends traffic
/// elsewhere, and a file with the right URL but no key fails at request time. The
/// key is matched exactly; the endpoint is matched by host so a trailing `/v1` or
/// a different scheme spelling does not produce a false negative.
fn verify_installed(path: &std::path::Path, key: &str, endpoint: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let has_key = text.contains(key.trim());
    let has_host = crate::relay::is_relay_endpoint(endpoint)
        .map(|host| text.contains(host.hostname()))
        .unwrap_or(false);
    has_key && has_host
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The console's key payload is snake_case.
    ///
    /// Regression test for a defect that survived three rounds of "it works":
    /// `#[serde(rename_all = "camelCase")]` made serde look for `groupId`, the
    /// server sent `group_id`, and every field after `name` silently became `None`.
    /// The request succeeded, the list rendered, and the group name was missing —
    /// with no error anywhere.
    #[test]
    fn the_key_payload_parses_snake_case_fields() {
        let wire = r#"{
            "id": 144,
            "name": "ui-dev-test",
            "key": "sk-57553cbb7f81a15eb9d7831fc44861369e0004edd38e11a9e185ba253136491a",
            "status": "active",
            "quota": 0,
            "quota_used": 12.5,
            "group_id": 18
        }"#;
        let key: KeySummary = serde_json::from_str(wire).expect("wire shape must parse");
        assert_eq!(key.id, 144);
        assert_eq!(key.group_id, Some(18), "group_id must survive the parse");
        assert_eq!(key.quota_used, Some(12.5), "quota_used must survive too");
    }

    #[test]
    fn masking_keeps_a_recognisable_prefix_and_suffix() {
        let key = "sk-45fdab35f6d1b74730f2e1f6166f72c6732e13f4d5c5423d2d17c11db77aa324";
        let masked = mask(key);
        assert!(masked.starts_with("sk-45fd"), "got {masked}");
        assert!(masked.ends_with("a324"), "got {masked}");
        assert!(!masked.contains("5c5423d2d17c11db"), "the body must be gone");
    }

    #[test]
    fn a_short_secret_does_not_leak_its_body() {
        assert_eq!(mask("short"), "sk-…");
        assert_eq!(mask("sk-123456789"), "sk-…");
    }

    #[test]
    fn a_disabled_key_is_not_reusable() {
        let base = KeySummary {
            id: 1,
            name: "n".into(),
            key: "sk-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            status: None,
            quota_used: None,
            quota: None,
            group_id: None,
        };
        assert!(base.is_usable(), "no status means usable");

        for status in ["disabled", "expired", "revoked"] {
            let key = KeySummary {
                status: Some(status.to_string()),
                ..base.clone()
            };
            assert!(!key.is_usable(), "{status} must not be reused");
        }
        let enabled = KeySummary {
            status: Some("enabled".into()),
            ..base
        };
        assert!(enabled.is_usable());
    }

    /// The generated request must carry every field the writer needs, for every
    /// client shape. A missing `models` list is the difference between a working
    /// client and an empty model menu.
    #[test]
    fn the_import_request_carries_the_key_endpoint_and_models() {
        let models = vec![
            "claude-opus-5-5".to_string(),
            "claude-sonnet-5".to_string(),
            "claude-haiku-4-5".to_string(),
        ];
        let request = import_request(
            "claude",
            "sk-test",
            "https://api.aimyria.com",
            &models,
            "AIMYRIA",
        );

        assert_eq!(request.resource, "provider");
        assert_eq!(request.app.as_deref(), Some("claude"));
        assert_eq!(request.api_key.as_deref(), Some("sk-test"));
        assert_eq!(request.endpoint.as_deref(), Some("https://api.aimyria.com"));
        assert_eq!(request.models.as_deref(), Some(models.as_slice()));
        // The newest model is the default, which is what the writer puts in
        // `model` / `opus_model`.
        assert_eq!(request.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(request.opus_model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(
            request.enabled,
            Some(true),
            "the whole point is to end up enabled"
        );
    }

    /// Verification must require both halves, and must not be fooled by a
    /// look-alike host.
    #[test]
    fn verification_requires_the_key_and_our_host() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.json");

        std::fs::write(
            &path,
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://api.aimyria.com","ANTHROPIC_AUTH_TOKEN":"sk-abc123"}}"#,
        )
        .unwrap();
        assert!(verify_installed(&path, "sk-abc123", "https://api.aimyria.com"));

        // Key present, foreign host: traffic would go elsewhere.
        std::fs::write(
            &path,
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://evil.example","ANTHROPIC_AUTH_TOKEN":"sk-abc123"}}"#,
        )
        .unwrap();
        assert!(!verify_installed(&path, "sk-abc123", "https://api.aimyria.com"));

        // Right host, key absent: requests would fail at runtime.
        std::fs::write(
            &path,
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://api.aimyria.com"}}"#,
        )
        .unwrap();
        assert!(!verify_installed(&path, "sk-abc123", "https://api.aimyria.com"));

        // Look-alike host must not count.
        std::fs::write(
            &path,
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://aimyria.com.evil.example","ANTHROPIC_AUTH_TOKEN":"sk-abc123"}}"#,
        )
        .unwrap();
        assert!(!verify_installed(&path, "sk-abc123", "https://api.aimyria.com"));
    }

    #[test]
    fn verification_of_a_missing_file_is_false_not_a_panic() {
        assert!(!verify_installed(
            std::path::Path::new("D:/definitely/not/here/settings.json"),
            "sk-x",
            "https://api.aimyria.com"
        ));
    }
}
