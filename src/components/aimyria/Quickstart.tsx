/**
 * Aimyria 接入中心 — the product's front door.
 *
 * Deliberately minimal: one sentence, one primary action, one compact list.
 * The previous version repeated the protocol, the config path and a badge on
 * every card; none of that helps the only decision this page exists for —
 * which client should this key configure.
 */

import { useCallback, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowUpRight, Loader2, RefreshCw } from "lucide-react";

import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { Button } from "@/components/ui/button";
import { AimyriaLogo } from "@/components/brand/AimyriaLogo";
import { AimyriaLockup } from "@/components/brand/AimyriaWordmark";
import { ProviderIcon } from "@/components/ProviderIcon";
import { fmtUsd } from "@/components/usage/format";
import { openExternal } from "@/lib/api/aimyria";
import {
  configureAll,
  configureOne,
  getClientStatus,
  getRelayBalance,
} from "@/lib/api/aimyria";
import type { ClientStatus, ConfigureReport } from "@/lib/types";
import {
  AIMYRIA_APPS,
  PRIMARY_AIMYRIA_APPS,
  findAimyriaApp,
  type AimyriaApp,
} from "@/shared/aimyriaApps";
import { cn } from "@/lib/utils";
import { useTauriEvent } from "@/hooks/useTauriEvent";

const LOW_BALANCE_USD = 2;
const CONSOLE_URL = "https://aimyria.com/keys";
const SITE_URL = "https://aimyria.com/go/";

const QUICKSTART_ICON: Record<string, string | undefined> = {
  claude: "claude",
  "claude-desktop": "claude",
  codex: "openai",
  gemini: "gemini",
  opencode: "opencode",
  openclaw: "openclaw",
  workbuddy: "workbuddy",
};

function statusFor(statuses: ClientStatus[], app: AimyriaApp) {
  return statuses.find((item) => item.app === app.id);
}

export function Quickstart() {
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState<string | null>(null);
  const [report, setReport] = useState<ConfigureReport | null>(null);
  const [showAll, setShowAll] = useState(false);

  const clients = useQuery({
    queryKey: ["aimyria-clients"],
    queryFn: () => getClientStatus(),
  });
  const balanceQuery = useQuery({
    queryKey: ["relay-balance"],
    queryFn: () => getRelayBalance(),
    refetchInterval: 60_000,
  });

  const refresh = useCallback(async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["aimyria-clients"] }),
      queryClient.invalidateQueries({ queryKey: ["relay-balance"] }),
    ]);
  }, [queryClient]);

  useTauriEvent("deeplink-import", () => {
    void refresh();
  });

  const run = useCallback(
    async (key: string, action: () => Promise<ConfigureReport>) => {
      setBusy(key);
      setReport(null);
      try {
        setReport(await action());
      } catch (error) {
        setReport({
          configured: [],
          failed: [[key, String(error)]],
          warnings: [],
        });
      } finally {
        setBusy(null);
        await refresh();
      }
    },
    [refresh],
  );

  const statuses = clients.data ?? [];
  const configuredCount = statuses.filter((item) => item.configured).length;
  const remaining = balanceQuery.data?.remaining ?? null;
  const balanceReason = balanceQuery.data?.reason ?? null;
  const balanceError = balanceQuery.data?.error ?? null;
  const low = remaining !== null && remaining < LOW_BALANCE_USD;
  const exhausted = remaining !== null && remaining <= 0;
  const writing = busy !== null;

  const balanceText =
    remaining !== null
      ? fmtUsd(remaining, 2)
      : balanceReason === "not_bound"
        ? "未导入密钥"
        : balanceReason === "bridge_unavailable"
          ? "余额待开通"
          : balanceReason === "query_failed"
            ? balanceError?.includes("过期")
              ? "令牌过期，请重新导入"
              : "暂时无法查询"
            : "—";

  const primaryRows = useMemo(
    () =>
      (showAll ? AIMYRIA_APPS : PRIMARY_AIMYRIA_APPS).map((app) => ({
        app,
        status: statusFor(statuses, app),
      })),
    [statuses, showAll],
  );

  return (
    <>
      <AppPageHeader
        icon={<AimyriaLogo size={18} />}
        title="接入中心"
        variant="page"
        actions={
          configuredCount > 0 ? (
            <Button
              variant="quiet"
              size="compact"
              disabled={writing}
              aria-busy={busy === "all"}
              onClick={() => void run("all", () => configureAll())}
            >
              {busy === "all" ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden />
              ) : null}
              重新应用
            </Button>
          ) : undefined
        }
      />

      <div className="min-h-0 flex-1 overflow-y-auto">
        <section className="mx-auto w-full max-w-[680px] px-6 pb-8 pt-10">
          <AimyriaLockup size={56} className="text-fg-1" />
          <h1 className="mt-5 text-[24px] font-semibold leading-tight tracking-[-0.015em] text-fg-1">
            一个密钥，接好所有 AI 工具
          </h1>
          <p className="mt-2 max-w-[460px] text-sm leading-6 text-fg-2">
            在 aimyria.com/keys 点「导入到 CCS」，选择应用后自动写入最新模型。
            密钥只保存在本机。
          </p>

          <div className="mt-5 flex flex-wrap items-center gap-3">
            <Button
              variant="solid"
              size="regular"
              onClick={() => void openExternal(CONSOLE_URL)}
            >
              打开密钥页
              <ArrowUpRight className="h-3.5 w-3.5" />
            </Button>
            <button
              type="button"
              onClick={() => void openExternal(SITE_URL)}
              className="text-caption text-fg-3 transition-colors hover:text-fg-1"
            >
              产品页 ↗
            </button>
          </div>

          <div className="mt-5 flex items-center gap-2 text-sm">
            <span className="text-fg-3">余额</span>
            <span
              className={cn(
                "tabular-nums font-medium",
                low || exhausted ? "text-danger-text" : "text-fg-1",
              )}
            >
              {balanceText}
            </span>
            <button
              type="button"
              aria-label="刷新余额"
              disabled={balanceQuery.isFetching}
              onClick={() => void balanceQuery.refetch()}
              className="rounded-full p-1 text-fg-3 transition-colors hover:bg-subtle hover:text-fg-1 disabled:opacity-50"
            >
              <RefreshCw
                className={cn(
                  "h-3.5 w-3.5",
                  balanceQuery.isFetching && "animate-spin",
                )}
              />
            </button>
            {low && remaining !== null ? (
              <span className="text-caption text-danger-text">余额偏低</span>
            ) : null}
          </div>
        </section>

        <section className="mx-auto w-full max-w-[680px] px-6 pb-12">
          <div className="flex items-center justify-between">
            <div className="text-caption text-fg-3">
              应用{" "}
              <span className="tabular-nums text-fg-2">
                {configuredCount}/{statuses.length}
              </span>
            </div>
            <button
              type="button"
              onClick={() => setShowAll((value) => !value)}
              className="text-caption text-fg-3 transition-colors hover:text-fg-1"
            >
              {showAll ? "只看常用" : "显示全部"}
            </button>
          </div>

          {clients.isLoading ? (
            <p className="py-10 text-center text-caption text-fg-3">正在读取…</p>
          ) : clients.isError ? (
            <p className="py-10 text-center text-caption text-danger-text">
              {String(clients.error)}
            </p>
          ) : (
            <div className="mt-2 divide-y divide-border overflow-hidden rounded-xl border border-border">
              {primaryRows.map(({ app, status }) => {
                const configured = status?.configured ?? false;
                const isBusy = busy === app.id;
                return (
                  <div
                    key={app.id}
                    className="flex items-center gap-3 px-4 py-3"
                  >
                    <ProviderIcon
                      icon={QUICKSTART_ICON[app.id]}
                      name={app.name}
                      size={18}
                      showFallback
                      className={
                        app.id === "codex" || app.id === "opencode"
                          ? "dark:invert"
                          : undefined
                      }
                    />
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-sm text-fg-1">
                        {app.name}
                      </div>
                      <div className="truncate text-[11px] text-fg-3">
                        {configured ? "已接入" : app.subtitle}
                      </div>
                    </div>
                    <Button
                      variant={configured ? "quiet" : "neutral"}
                      size="compact"
                      disabled={writing}
                      onClick={() =>
                        void run(app.id, () => configureOne(app.id))
                      }
                    >
                      {isBusy ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : null}
                      {configured ? "重新应用" : "接入"}
                    </Button>
                  </div>
                );
              })}
            </div>
          )}

          {report?.failed.map(([app, reason]) => (
            <p key={app} className="mt-2 text-caption text-danger-text">
              {findAimyriaApp(app)?.name ?? app}：{reason}
            </p>
          ))}
          {report?.warnings.map((warning, index) => (
            <p key={index} className="mt-2 text-caption text-fg-2">
              {warning}
            </p>
          ))}
          {report &&
          report.failed.length === 0 &&
          report.warnings.length === 0 ? (
            <p className="mt-2 text-caption text-success-text">
              已应用 {report.configured.length} 个应用
            </p>
          ) : null}

          <p className="mt-4 text-[11px] text-fg-3">
            密钥只保存在本机 · 只写你选择的应用
          </p>
        </section>
      </div>
    </>
  );
}
