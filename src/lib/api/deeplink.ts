import { invoke } from "@tauri-apps/api/core";

export type ResourceType = "provider" | "prompt" | "mcp" | "skill";

export interface DeepLinkImportRequest {
  version: string;
  resource: ResourceType;
  /**
   * Original URL, attached by the backend listener.
   *
   * The parsed fields are what the dialog displays; the URL is kept only so a
   * "copy link" action can hand the same request to another machine.
   */
  rawUrl?: string;

  // Common fields
  app?:
    | "claude"
    | "claude-desktop"
    | "codex"
    | "gemini"
    | "grokbuild"
    | "opencode"
    | "openclaw"
    | "hermes"
    | "pi"
    | "workbuddy"
    | "dsh";
  name?: string;
  enabled?: boolean;

  // Provider fields
  homepage?: string;
  endpoint?: string;
  apiKey?: string;
  icon?: string;
  model?: string;
  notes?: string;
  haikuModel?: string;
  sonnetModel?: string;
  opusModel?: string;
  /** All models the key's group owns, so every slot can be filled at once. */
  models?: string[];
  /** Human-readable relay group; shown in the confirmation dialog. */
  groupName?: string;
  /** Short-lived read-only balance bridge minted by the console. */
  balanceToken?: string;
  balanceUrl?: string;

  // Prompt fields
  content?: string;
  description?: string;

  // MCP fields
  apps?: string; // Comma-separated application IDs

  // Skill fields
  repo?: string;
  directory?: string;
  branch?: string;

  // Config file fields
  config?: string;
  configFormat?: string;
  configUrl?: string;

  // Usage script fields (v3.9+)
  usageEnabled?: boolean;
  usageScript?: string;
  usageApiKey?: string;
  usageBaseUrl?: string;
  usageAccessToken?: string;
  usageUserId?: string;
  usageAutoInterval?: number;
}

export interface McpImportResult {
  importedCount: number;
  importedIds: string[];
  failed: Array<{
    id: string;
    error: string;
  }>;
}

export type ImportResult =
  | { type: "provider"; id: string }
  | { type: "prompt"; id: string }
  | {
      type: "mcp";
      importedCount: number;
      importedIds: string[];
      failed: Array<{ id: string; error: string }>;
    }
  | { type: "skill"; key: string };

export const deeplinkApi = {
  /**
   * Claim the deep link that arrived before this dialog could listen for it.
   *
   * A deep link that *starts* the app is parsed during `setup()`, while the
   * webview is still loading and no `listen()` exists yet. Tauri drops events
   * with no listener, so the window would open with no confirmation and the
   * import would never happen. The backend keeps the last such request; this
   * takes it (destructively) so it can be shown once, and never replayed on a
   * later launch.
   */
  takePendingDeeplink: async (): Promise<DeepLinkImportRequest | null> => {
    return invoke("take_pending_deeplink");
  },

  /**
   * Parse a deep link URL
   * @param url The ccswitch:// URL to parse
   * @returns Parsed deep link request
   */
  parseDeeplink: async (url: string): Promise<DeepLinkImportRequest> => {
    return invoke("parse_deeplink", { url });
  },

  /**
   * Merge configuration from Base64/URL into a deep link request
   * This is used to show the complete configuration in the confirmation dialog
   * @param request The deep link import request
   * @returns Merged deep link request with config fields populated
   */
  mergeDeeplinkConfig: async (
    request: DeepLinkImportRequest,
  ): Promise<DeepLinkImportRequest> => {
    return invoke("merge_deeplink_config", { request });
  },

  /**
   * Import a resource from a deep link request (unified handler)
   * @param request The deep link import request
   * @returns Import result based on resource type
   */
  importFromDeeplink: async (
    request: DeepLinkImportRequest,
  ): Promise<ImportResult> => {
    return invoke("import_from_deeplink_unified", { request });
  },
};
