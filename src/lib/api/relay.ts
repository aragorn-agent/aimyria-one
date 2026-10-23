/**
 * Typed wrappers for the console session commands.
 *
 * Mirrors `commands/relay.rs`. Nothing here returns a token: the backend keeps
 * them, and this module only ever learns whether a session exists.
 */

import { invoke } from "@tauri-apps/api/core";

/** Machine-readable failure from the relay layer. */
export interface RelayCommandError {
  /** `offline` | `needs_reauth` | `rejected` | `server_error` | `invalid_input` */
  code: string;
  /** A sentence meant for a person. */
  message: string;
}

export interface RelaySessionStatus {
  signedIn: boolean;
  account: string;
  refreshExpiresAt: number | null;
  /**
   * The stored session could not be used. The UI should show the login form with
   * an explanation rather than pretending the user was never signed in.
   */
  needsReauth: boolean;
  /**
   * A session file exists but the server has not confirmed it yet.
   *
   * Happens after a restore-from-backup, a copied profile, or a leftover file from
   * another build. The UI should say "正在验证…" rather than "已登录", because the
   * app cannot yet act for that account.
   */
  unverified: boolean;
}

export function relaySessionStatus(): Promise<RelaySessionStatus> {
  return invoke<RelaySessionStatus>("relay_session_status");
}

/** Sign in. Resolves on success; rejects with a `RelayCommandError` otherwise. */
export function relayLogin(
  account: string,
  password: string,
): Promise<void> {
  return invoke<void>("relay_login", { account, password });
}

/** Sign out on this machine. Always clears local state, even offline. */
export function relayLogout(): Promise<void> {
  return invoke<void>("relay_logout");
}

/** Force a refresh; used when the machine woke from sleep with a stale token. */
export function relayRefreshSession(): Promise<RelaySessionStatus> {
  return invoke<RelaySessionStatus>("relay_refresh_session");
}

// ── keys ───────────────────────────────────────────────────────────────────

/** A key as the console describes it. Never carries plaintext. */
export interface RelayKey {
  id: number;
  name: string;
  /** `sk-…abcd` — display only. The plaintext never leaves Rust. */
  masked: string;
  status?: string | null;
  used?: number | null;
  quota?: number | null;
  groupId?: number | null;
  /** Human name of the key's group, e.g. 「国模·稳定」. */
  groupName?: string | null;
  /** One-line model summary the console publishes for that group. */
  groupModels?: string | null;
}

/** Result of installing a key into one client. */
export interface RelayInstallReport {
  app: string;
  /** `created` when a new key was minted, `rotated` when a rotation installed it. */
  keySource: string;
  keyMasked: string;
  writtenPath: string | null;
  /** False means the file was written but read-back did not confirm it. */
  verified: boolean;
  warnings: string[];
}

export function relayKeysList(): Promise<RelayKey[]> {
  return invoke<RelayKey[]>("relay_keys_list");
}

/**
 * Create a key, install it into one client, and verify the write.
 *
 * One command on purpose: splitting create from install would let the UI show a
 * half-finished state, and would make "a key was created but the config was not
 * written" a thing the frontend has to reason about.
 */
export function relayKeyInstall(
  app: string,
  options: { endpoint?: string; models?: string[]; group?: string } = {},
): Promise<RelayInstallReport> {
  return invoke<RelayInstallReport>("relay_key_install", {
    app,
    endpoint: options.endpoint ?? null,
    models: options.models ?? null,
    group: options.group ?? null,
  });
}

/** Rotate a key and re-install it into the given clients. */
export function relayKeyRotate(
  keyId: number,
  apps: string[],
): Promise<RelayInstallReport[]> {
  return invoke<RelayInstallReport[]>("relay_key_rotate", { keyId, apps });
}

/** Disable a key. Revokes access immediately, keeps the row restorable. */
export function relayKeyDisable(keyId: number): Promise<void> {
  return invoke<void>("relay_key_disable", { keyId });
}

// ── overview ───────────────────────────────────────────────────────────────

export interface RelayBalanceBlock {
  remaining: number | null;
  used: number | null;
  planName: string | null;
  error: string | null;
  reason: string | null;
}

export interface RelayUsageBlock {
  todayRequests: number;
  todayCost: string;
  todayTokens: number;
  monthRequests: number;
  monthCost: string;
  successRate: number;
  error: string | null;
}

export interface RelayModelsBlock {
  groupName: string;
  count: number;
  sample: string[];
  error: string | null;
}

export interface RelayClientsBlock {
  configured: number;
  total: number;
  names: string[];
}

export interface RelayOverview {
  balance: RelayBalanceBlock;
  usage: RelayUsageBlock;
  models: RelayModelsBlock;
  clients: RelayClientsBlock;
  fetchedAt: number;
}

/**
 * Everything the overview page shows.
 *
 * One command rather than four: each block reports its own failure, so a broken
 * model list does not blank the balance, and the page has one loading state.
 */
export function relayOverview(): Promise<RelayOverview> {
  return invoke<RelayOverview>("relay_overview");
}

/** What the user should do, in their words, for a given failure code. */
export function relayErrorHint(error: RelayCommandError): string {
  switch (error.code) {
    case "offline":
      return "检查网络后重试。已接入的客户端不受影响，它们的密钥仍然有效。";
    case "needs_reauth":
      return "会话已结束，请重新登录。已接入的客户端不受影响。";
    case "server_error":
      return "中转站暂时不可用，稍后再试。";
    case "invalid_input":
      return "请填写完整的账号和密码。";
    default:
      return "";
  }
}

// ── credentials (keys as resources) ────────────────────────────────────────

/** One key this machine knows, with the clients currently bound to it. */
export interface RelayCredential {
  /** Hash of the secret. Stable React key; never the secret itself. */
  fingerprint: string;
  /** `sk-45fd…a324` */
  masked: string;
  endpoint: string;
  name: string;
  /** Whether the endpoint is one of our relays. */
  isRelay: boolean;
  /** `AppType::as_str()` of every client bound to this key. */
  boundApps: string[];
}

/** One client and the key currently in its live config. */
export interface RelayAppBinding {
  app: string;
  label: string;
  fingerprint: string | null;
  masked: string | null;
  bound: boolean;
}

export interface RelayCredentialMatrix {
  credentials: RelayCredential[];
  apps: RelayAppBinding[];
}

/**
 * Every key this machine knows, and what each client is bound to.
 *
 * There is no console endpoint that lists an account's keys, so this cannot show
 * keys created on the website but never imported here. It shows the set the user
 * can actually bind something to.
 */
export function relayCredentials(): Promise<RelayCredentialMatrix> {
  return invoke<RelayCredentialMatrix>("relay_credentials");
}

/** Bind one client to a key, by fingerprint. */
export function relayBind(app: string, keyFingerprint: string): Promise<void> {
  return invoke<void>("relay_bind", { app, keyFingerprint });
}

/** Result of bringing Claude Desktop onto a freshly written configuration. */
export interface DesktopRestart {
  wasRunning: boolean;
  relaunched: boolean;
  /** What to tell the user when `relaunched` is false. */
  hint: string | null;
}

/**
 * Restart Claude Desktop so it loads the configuration just written.
 *
 * Claude Desktop reads its profile only at startup. The configuration is already on
 * disk by the time this is called; this is what turns it into something the user
 * can immediately use, instead of asking them to quit and reopen it themselves.
 */
export function relayRestartClaudeDesktop(): Promise<DesktopRestart> {
  return invoke<DesktopRestart>("relay_restart_claude_desktop");
}
