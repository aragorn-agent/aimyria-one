/**
 * Wire types shared with the Rust command layer.
 *
 * These mirror the `serde(rename_all = "camelCase")` structs in
 * `src-tauri/src/commands/aimyria.rs`. Keep the two in step: a rename on the
 * Rust side silently becomes `undefined` here, which is why the UI treats every
 * field defensively rather than trusting it.
 */

/**
 * One client identifier, as the backend spells it.
 *
 * Copied verbatim from upstream's `lib/api/types.ts`, including the five this
 * product does not configure (claude-desktop, grokbuild, hermes, pi, mcode).
 *
 * That is deliberate. This alias describes `AppType` in `app_config.rs`, which
 * has ten variants and which the skills and prompts commands genuinely accept —
 * narrowing it to five would mean rewriting upstream's `skills.ts` and
 * `prompts.ts` line by line, and those files are copied unmodified. The UI
 * simply never offers the other five; the type saying they exist is the
 * accurate description, not a promise that this build exposes them.
 */
export type AppId =
  | "claude"
  | "claude-desktop"
  | "codex"
  | "gemini"
  | "grokbuild"
  | "opencode"
  | "openclaw"
  | "hermes"
  | "pi"
  | "mcode";

/** One client the tool can configure. */
export interface ClientStatus {
  /** `AppType::as_str()` — stable across releases, safe as a React key. */
  app: string;
  /** Display name, e.g. "Claude Code". */
  label: string;
  /** Whether this build can write the client's config at all. */
  supported: boolean;
  /** Whether the client's executable is on PATH. */
  cliInstalled: boolean;
  /** Config file this build would write. */
  livePath: string | null;
  /** Whether the live config already points at our relay. */
  configured: boolean;
  /** The provider row currently selected, if any. */
  currentProvider: string | null;
}

/**
 * The relay balance.
 *
 * `remaining` is `null` when the balance could not be read — that is different
 * from `0`, and the UI must say "could not refresh" rather than "$0.00".
 */
export interface RelayBalance {
  remaining: number | null;
  used: number | null;
  low: boolean;
  error: string | null;
  /**
   * Machine-readable reason when `remaining` is null.
   *
   * `not_bound` = no account yet; `bridge_unavailable` = the relay has no
   * key-scoped balance endpoint; `query_failed` = the bridge exists but the
   * request failed. Kept separate so the UI can tell the user what to do
   * instead of showing a generic dash.
   */
  reason?: "not_bound" | "bridge_unavailable" | "query_failed" | null;
}

/** What a configure run wrote, and what it could not. */
export interface ConfigureReport {
  configured: string[];
  failed: [string, string][];
  warnings: string[];
}

/** Static facts for About and outbound links. */
export interface ProductInfo {
  name: string;
  version: string;
  website: string;
  docs: string;
  support: string;
  license: string;
  /** Must be shown: this is a derivative work under MIT. */
  attribution: string;
}

// ── 用量统计 ──────────────────────────────────────────────────────────────
// Mirrors the `serde(rename_all = "camelCase")` structs in
// `src-tauri/src/services/usage_stats.rs`.
//
// Every number here is what was actually recorded. With direct connections the
// tool does not see request traffic — the client talks to the relay itself — so
// these tables stay empty until the relay's own logs are imported or a session
// scanner is restored. The dashboard says so rather than showing a zero-filled
// chart that looks like real data.

export interface UsageSummary {
  totalRequests: number;
  /** Cost as a decimal string, kept as text so no rounding is lost in transit. */
  totalCost: string;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheCreationTokens: number;
  totalCacheReadTokens: number;
  /** 0.0–1.0. */
  successRate: number;
  /** input + output + cache_creation + cache_read. */
  realTotalTokens: number;
  /** cache_read / cacheable input. 0.0–1.0. */
  cacheHitRate: number;
}

export interface DailyStats {
  date: string;
  requestCount: number;
  totalCost: string;
  totalTokens: number;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheCreationTokens: number;
  totalCacheReadTokens: number;
}

export interface ProviderStats {
  providerId: string;
  appType: string;
  providerName: string;
  requestCount: number;
  totalTokens: number;
  totalCost: string;
  /** 0.0–1.0. */
  successRate: number;
  avgLatencyMs: number;
}

export interface ModelStats {
  model: string;
  requestCount: number;
  totalTokens: number;
  totalCost: string;
  avgCostPerRequest: string;
  /** 0.0–1.0. */
  successRate: number;
}

export interface RequestLogDetail {
  requestId: string;
  providerId: string;
  providerName?: string;
  appType: string;
  model: string;
  requestModel?: string;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  statusCode: number;
  createdAt: number;
  durationMs?: number;
  cost?: string;
}

export interface LogFilters {
  appType?: string | null;
  providerName?: string | null;
  model?: string | null;
  statusCode?: number | null;
  startDate?: number | null;
  endDate?: number | null;
}

export interface PaginatedLogs {
  data: RequestLogDetail[];
  total: number;
}
