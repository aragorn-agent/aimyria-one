import { invoke } from "@tauri-apps/api/core";

/**
 * MCP servers — one row per server, plus which clients it is enabled in.
 *
 * The payload crosses the IPC boundary as the `McpServer` serde struct, so
 * `apps` is a map keyed by client (`claude`, `codex`, ...) rather than the flat
 * booleans the older API used. That is the shape the backend actually stores:
 * a server can be enabled in three clients and disabled in two.
 */
/**
 * Which clients hold this server.
 *
 * Mirrors `McpApps` in `app_config.rs` — a struct of named booleans, not a map.
 * The backend writes every one of these into its own config file, so the keys
 * are fixed and unknown ones cannot appear.
 *
 * `Record<McpClient, boolean>` rather than the eight-field struct: this product
 * configures five clients, and the three upstream fields it does not touch
 * (mcode, grokbuild, hermes, pi) are better absent than present-and-false — a
 * column in the UI for a client we do not support is a column that can never be
 * checked.
 */
export type McpClient = "claude" | "codex" | "gemini" | "opencode" | "openclaw";

export type McpApps = Record<McpClient, boolean>;

export interface McpServer {
  id: string;
  name: string;
  /** The server definition itself — command, args, env, exactly as stored. */
  server: Record<string, unknown>;
  apps: McpApps;
  description?: string | null;
  homepage?: string | null;
  docs?: string | null;
  tags?: string[];
}

export const mcp = {
  list: () => invoke<McpServer[]>("get_mcp_servers"),

  /** Add or update. Returns the saved row, with `apps` as persisted. */
  save: (server: McpServer) => invoke<McpServer>("upsert_mcp_server", { server }),

  remove: (id: string) => invoke<boolean>("delete_mcp_server", { id }),

  /** Enable or disable this server in one client, writing that client's file. */
  setEnabled: (id: string, app: string, enabled: boolean) =>
    invoke<boolean>("set_mcp_enabled", { id, app, enabled }),

  /** Pick up servers already configured inside each client's own config. */
  importFromApps: () => invoke<number>("import_mcp_from_apps"),

  /** Rewrite every client's file from the database. */
  resync: (apps?: string[]) => invoke<unknown[]>("resync_mcp_to_apps", { apps: apps ?? null }),
};

/** Clients this product configures, in display order. */
export const MCP_CLIENTS = ["claude", "codex", "gemini", "opencode", "openclaw"] as const;

export const CLIENT_LABELS: Record<McpClient, string> = {
  claude: "Claude Code",
  codex: "Codex",
  gemini: "Gemini CLI",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
};