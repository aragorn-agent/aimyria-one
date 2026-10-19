import { useMemo } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
import type { AppId } from "@/lib/api";
import { getRelayBalance } from "@/lib/api/aimyria";
import * as authApi from "@/lib/api/auth";
import type { ManagedAuthProvider } from "@/lib/api/auth";
import { useProvidersQuery } from "@/lib/query";
import { useProxyStatusQuery } from "@/lib/query/proxy";
import { proxyApi } from "@/lib/api/proxy";
import { useUsageSummary } from "@/lib/query/usage";
import { PROXY_APP_IDS, isProxyAppId } from "@/config/appConfig";
import { providerNeedsRouting } from "@/utils/providerCapabilities";
import { parseFiniteNumber } from "@/components/usage/format";
import type { UsageRangeSelection } from "@/types/usage";
import type { AppMode } from "@/types/proxy";

export interface AppNavStatus {
  mode: AppMode;
  /** Claude Desktop 的当前供应商走模型映射（需要路由服务） */
  mapping: boolean;
  /** 需要处理：在路由 / 聚合 / 映射，但路由服务没在运行 */
  alert: boolean;
}

const TODAY: UsageRangeSelection = { preset: "today" };
const MANAGED_AUTH_PROVIDERS: ManagedAuthProvider[] = [
  "github_copilot",
  "codex_oauth",
  "xai_oauth",
];

/**
 * 侧栏需要的状态：每个应用的模式标签和提醒、今日花费、授权中心是否有账号要重新登录。
 */
export function useSidebarStatus() {
  const { data: proxyStatus } = useProxyStatusQuery();
  // 和应用页的模式行同一份数据（["providers", app, "mode"]），进出模式时一起刷新
  const modeQueries = useQueries({
    queries: PROXY_APP_IDS.map((app) => ({
      queryKey: ["providers", app, "mode"],
      queryFn: () => proxyApi.getAppMode(app),
    })),
  });
  const { data: desktopProviders } = useProvidersQuery("claude-desktop");
  const { data: todaySummary } = useUsageSummary(TODAY, undefined, {
    refetchInterval: 60_000,
  });

  // 中转站余额。侧栏「一键配置」那一行的右侧数字就是它。
  //
  // 走的是上游同一套 react-query 约定，刻意不新造一套：窗口开着的时候这一行
  // 每分钟刷新一次，和今日花费同一个节奏。
  const balanceQuery = useQuery({
    queryKey: ["relay-balance"],
    queryFn: () => getRelayBalance(),
    refetchInterval: 60_000,
  });
  const balance = balanceQuery.data?.remaining ?? null;

  const authStatuses = useQueries({
    queries: MANAGED_AUTH_PROVIDERS.map((provider) => ({
      queryKey: ["managed-auth-status", provider],
      queryFn: () => authApi.authGetStatus(provider),
      staleTime: 60_000,
    })),
  });

  const serviceRunning = proxyStatus?.running ?? false;
  const modes = PROXY_APP_IDS.map((_, index) => modeQueries[index]?.data?.mode);
  const modeKey = modes.join(",");

  const desktopMapping = useMemo(() => {
    const current =
      desktopProviders?.providers[desktopProviders.currentProviderId];
    return current ? providerNeedsRouting("claude-desktop", current) : false;
  }, [desktopProviders]);

  const appStatus = useMemo(() => {
    return (app: AppId): AppNavStatus => {
      if (app === "claude-desktop") {
        return {
          mode: "direct",
          mapping: desktopMapping,
          alert: desktopMapping && proxyStatus !== undefined && !serviceRunning,
        };
      }
      if (!isProxyAppId(app)) {
        return { mode: "direct", mapping: false, alert: false };
      }
      const mode = modes[PROXY_APP_IDS.indexOf(app)] ?? "direct";
      return {
        mode,
        mapping: false,
        alert:
          mode !== "direct" && proxyStatus !== undefined && !serviceRunning,
      };
    };
    // modes 是每次渲染新建的数组，用 modeKey 判断变化
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [desktopMapping, proxyStatus, serviceRunning, modeKey]);

  const todayCost = parseFiniteNumber(todaySummary?.totalCost) ?? 0;

  const authNeedsAttention = authStatuses.some((query) =>
    query.data?.accounts.some(
      (account) => account.requires_reauth || account.reauth_required === true,
    ),
  );

  return { appStatus, todayCost, authNeedsAttention, balance };
}
