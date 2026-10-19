// Restore the cloud-sync settings surface into settings.rs.
//
// M2 removed WebDAV/S3 sync, leaving the `WebDavSyncSettings` type behind but no
// `S3SyncSettings` type, no fields on `AppSettings`, and none of the six
// accessors. Copying `services/webdav_sync` and `services/s3_sync` back (M21/M22)
// needs all three.
//
// Everything inserted here is upstream's, verbatim:
//   - `S3SyncSettings` + Default + impl  <- upstream settings.rs lines 190-288
//   - two fields on `AppSettings`        <- upstream lines 509-511
//   - two defaults                       <- upstream lines 605-606
//   - six accessor functions             <- upstream lines 1225-1266
//
// Idempotent: running it twice is a no-op.

import { readFileSync, writeFileSync } from "node:fs";

const FILE = "D:/src/cc-switch-main/src-tauri/src/settings.rs";
const UP = "D:/src/cc-switch-upstream/src-tauri/src/settings.rs";
let t = readFileSync(FILE, "utf8");
const up = readFileSync(UP, "utf8");

const applied = [];

if (!t.includes("pub struct S3SyncSettings")) {
  // Upstream lines 190-288, the whole S3SyncSettings block.
  const block = up
    .split("\n")
    .slice(189, 288)
    .join("\n");
  // Anchor: append after the WebDavSyncSettings impl, just before whatever
  // follows it. The end of that impl is the `is_empty` helper closing it.
  const anchor = "impl WebDavSyncSettings {";
  const start = t.indexOf(anchor);
  if (start === -1) {
    console.error("anchor `impl WebDavSyncSettings {` not found");
    process.exit(1);
  }
  // Find the closing brace of that impl block: first "\n}\n" after start.
  const end = t.indexOf("\n}\n", start);
  if (end === -1) {
    console.error("could not find the end of `impl WebDavSyncSettings`");
    process.exit(1);
  }
  const at = end + 3;
  t = `${t.slice(0, at)}${block}\n${t.slice(at)}`;
  applied.push("S3SyncSettings type");
} else {
  applied.push("S3SyncSettings already present");
}

if (!/\n\s+pub webdav_sync: Option<WebDavSyncSettings>,/.test(t)) {
  const anchor = "    // ===== 备份策略设置 =====";
  if (!t.includes(anchor)) {
    console.error("field anchor `// ===== 备份策略设置 =====` not found");
    process.exit(1);
  }
  const fields = [
    "    // ===== WebDAV / S3 同步设置 =====",
    "    #[serde(default, skip_serializing_if = \"Option::is_none\")]",
    "    pub webdav_sync: Option<WebDavSyncSettings>,",
    "    #[serde(default, skip_serializing_if = \"Option::is_none\")]",
    "    pub s3_sync: Option<S3SyncSettings>,",
    "",
    "",
  ].join("\n");
  t = t.replace(anchor, `${fields}${anchor}`);
  applied.push("AppSettings fields");
} else {
  applied.push("AppSettings fields already present");
}

if (!/^\s+s3_sync: None,$/m.test(t)) {
  const anchor = "            backup_interval_hours: None,";
  if (!t.includes(anchor)) {
    console.error("default anchor `backup_interval_hours: None,` not found");
    process.exit(1);
  }
  t = t.replace(
    anchor,
    `            webdav_sync: None,\n            s3_sync: None,\n${anchor}`,
  );
  applied.push("AppSettings defaults");
} else {
  applied.push("AppSettings defaults already present");
}

if (!t.includes("pub fn get_webdav_sync_settings")) {
  // Upstream lines 1225-1266.
  const accessors = up
    .split("\n")
    .slice(1224, 1266)
    .join("\n");
  t = `${t.trimEnd()}\n\n// ===== WebDAV / S3 同步设置管理函数 =====\n//\n// Upstream lines 1225-1266, verbatim.\n${accessors}\n`;
  applied.push("six accessors");
} else {
  applied.push("accessors already present");
}

writeFileSync(FILE, t);
console.log("applied:");
for (const a of applied) console.log(`  ${a}`);
