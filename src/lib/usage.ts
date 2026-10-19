import { invoke } from "@tauri-apps/api/core";

import type {
  DailyStats,
  LogFilters,
  ModelStats,
  PaginatedLogs,
  ProviderStats,
  UsageSummary,
} from "../lib/types";

/**
 * Usage statistics — thin wrappers over the `usage_stats` service.
 *
 * The backend has been in the tree, unchanged, since upstream; it was simply
 * never reachable. These calls are what make it reachable.
 *
 * Nothing here invents a shape: every payload comes from the Rust structs
 * verbatim, so the dashboard shows what was actually recorded rather than what
 * the UI wished had been recorded.
 */
export const usage = {
  summary: (appType?: string) =>
    invoke<UsageSummary>("get_usage_summary", { appType: appType ?? null }),

  summaryByApp: () => invoke<UsageSummaryByApp[]>("get_usage_summary_by_app"),

  trends: (days = 7, appType?: string) =>
    invoke<DailyStats[]>("get_usage_trends", {
      days,
      appType: appType ?? null,
    }),

  firstDate: () => invoke<string | null>("get_usage_first_date"),

  providers: () => invoke<ProviderStats[]>("get_provider_stats"),

  models: () => invoke<ModelStats[]>("get_model_stats"),

  logs: (filters: LogFilters, page: number, pageSize: number) =>
    invoke<PaginatedLogs>("get_request_logs", { filters, page, pageSize }),
};

type UsageSummaryByApp = { appType: string; summary: UsageSummary };