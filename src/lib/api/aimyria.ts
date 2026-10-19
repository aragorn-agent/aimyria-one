/**
 * Typed wrappers over the Tauri command surface.
 *
 * Every function here maps to exactly one `#[tauri::command]` in
 * `src-tauri/src/commands/aimyria.rs`. Nothing else in the app calls `invoke`
 * directly, so this file is the whole front/back contract.
 */

import { invoke } from "@tauri-apps/api/core";

import type {
  ClientStatus,
  ConfigureReport,
  ProductInfo,
  RelayBalance,
} from "@/lib/types";

/** The relay console, where an account is created and its key issued. */
export const CONSOLE_URL = "https://aimyria.com/console";

/** Status of every client the build knows about. */
export function getClientStatus(): Promise<ClientStatus[]> {
  return invoke<ClientStatus[]>("get_client_status");
}

/** Re-write every client that has a bound account. */
export function configureAll(): Promise<ConfigureReport> {
  return invoke<ConfigureReport>("configure_all");
}

/** Re-write one client. `app` is `AppType::as_str()`. */
export function configureOne(app: string): Promise<ConfigureReport> {
  return invoke<ConfigureReport>("configure_one", { app });
}

/** The relay balance, or the reason there is no number. */
export function getRelayBalance(): Promise<RelayBalance> {
  return invoke<RelayBalance>("get_relay_balance");
}

export interface RelayModels {
  groupName: string;
  platform: string;
  models: string[];
}

/** Read the group whitelist for a key, through the desktop backend. */
export function getRelayModels(
  baseUrl: string,
  apiKey: string,
): Promise<RelayModels> {
  return invoke<RelayModels>("get_relay_models", { baseUrl, apiKey });
}

export function getAutostart(): Promise<boolean> {
  return invoke<boolean>("get_autostart");
}

export function setAutostart(enabled: boolean): Promise<void> {
  return invoke<void>("set_autostart", { enabled });
}

export function getProductInfo(): Promise<ProductInfo> {
  return invoke<ProductInfo>("product_info");
}

/** Open a URL in the user's browser. */
export function openExternal(url: string): Promise<void> {
  return invoke<void>("open_external", { url });
}

/** Hide the window — the tool lives in the tray. */
export function hideWindow(): Promise<void> {
  return invoke<void>("hide_main_window");
}
