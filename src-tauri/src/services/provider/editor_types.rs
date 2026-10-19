//! Provider editor data shapes, split out from the upstream editor engine.
//!
//! Upstream's `claude_editor.rs` computed a field-level diff against the
//! client's live config, then handed the diff to the `mode/` write engine.
//! That engine is gone, and with it the diff machinery. The Tauri commands
//! still need these types on the wire, so they live here as plain data, and
//! the projection logic is rebuilt in phase 2.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What the editor shows for one provider.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorView {
    /// The full config the editor displays.
    pub settings: Value,
    /// Fields stored on the row that a switch does not apply.
    pub inactive: Vec<InactiveField>,
}

/// A row-stored field whose value differs from what a switch would show.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InactiveField {
    /// Key path, e.g. `["env", "API_TIMEOUT_MS"]` or `["hooks"]`.
    pub path: Vec<String>,
    pub value: Value,
}

/// What to do when the live config changed underneath an open editor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicy {
    /// Report the conflict and write nothing.
    #[default]
    Refuse,
    /// Overwrite with the editor's value.
    KeepMine,
    /// Keep the external value for conflicting keys; apply the rest.
    KeepTheirs,
}

impl ConflictPolicy {
    /// Decide which changes may be written, given what is on disk now.
    ///
    /// A change conflicts when the live value differs both from what the editor
    /// started from *and* from what it would write — i.e. something else
    /// changed that key while the editor was open.
    pub fn resolve<T, F>(
        &self,
        changes: &[T],
        mut conflict_of: F,
    ) -> Result<(), Vec<String>>
    where
        F: FnMut(&T) -> Option<String>,
    {
        let conflicts: Vec<String> = changes.iter().filter_map(&mut conflict_of).collect();
        match self {
            ConflictPolicy::Refuse if !conflicts.is_empty() => Err(conflicts),
            _ => Ok(()),
        }
    }
}

/// A save coming from the editor UI.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSave {
    /// The full config as displayed when the editor opened.
    pub base: Value,
    /// For add dialogs: the draft the display was projected from.
    #[serde(default)]
    pub draft: Option<Value>,
    #[serde(default)]
    pub on_conflict: ConflictPolicy,
}