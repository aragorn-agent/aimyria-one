/**
 * 接入中心 — one screen for keys and the clients bound to them.
 *
 * # What changed, and why
 *
 * The previous version was client-first: six rows, each with its own buttons, and
 * the key about to be used was invisible. The user's actual question is "which key
 * is this client on, and can I move it" — so this screen is key-first:
 *
 *   * one row per client whose control is a **key picker**, not a button;
 *   * every key this machine knows, with the clients on it;
 *   * binding writes through the same switch the provider page uses, so the two
 *     screens cannot disagree — whichever side you change, the other reflects it.
 *
 * # The rule about text
 *
 * One short line per area. Anything longer is either a real problem (state
 * mismatch, write failure) or it does not belong here. Each sentence that survives
 * earns its space by telling the user what to do, not how the feature works.
 */

import { useCallback, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleAlert, Loader2, RefreshCw } from "lucide-react";

import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { Button } from "@/components/ui/button";
import { AimyriaLogo } from "@/components/brand/AimyriaLogo";
import { ProviderIcon } from "@/components/ProviderIcon";
import { fmtUsd } from "@/components/usage/format";
import {
  configureOne,
  getClientStatus,
  getRelayBalance,
  openExternal,
  revertToOfficial,
} from "@/lib/api/aimyria";
import {
  relayBind,
  relayCredentials,
  relayKeysList,
  relayOverview,
  relayRestartClaudeDesktop,
  type RelayCredentialMatrix,
  type RelayKey,
} from "@/lib/api/relay";
import { RelayLoginDialog } from "@/components/aimyria/RelayLoginDialog";
import { useRelaySession } from "@/hooks/useRelaySession";
import type { ClientStatus } from "@/lib/types";
import { AIMYRIA_APPS, PRIMARY_AIMYRIA_APPS } from "@/shared/aimyriaApps";
import { RelayModelPicker } from "@/components/aimyria/RelayModelPicker";
import { cn } from "@/lib/utils";
import { useTauriEvent } from "@/hooks/useTauriEvent";

const CONSOLE_URL = "https://aimyria.com/keys";

const APP_ICON: Record<string, string | undefined> = {
  claude: "claude",
  "claude-desktop": "claude",
  codex: "openai",
  gemini: "gemini",
  opencode: "opencode",
  openclaw: "openclaw",
  workbuddy: "workbuddy",
  hermes: undefined,
  dsh: undefined,
};

function fmtCost(value: string | undefined): string {
  const parsed = Number(value ?? "0");
  return Number.isFinite(parsed) ? fmtUsd(parsed, 2) : "—";
}

export function Quickstart() {
  const queryClient = useQueryClient();
  const [busyApp, setBusyApp] = useState<string | null>(null);
  const [pickerApp, setPickerApp] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loginOpen, setLoginOpen] = useState(false);
  // What Claude Desktop needs from the user after a binding, when we could not do
  // it for them.
  const [desktopHint, setDesktopHint] = useState<string | null>(null);
  const { isSignedIn, account, unverified } = useRelaySession();

  const clients = useQuery({
    queryKey: ["aimyria-clients"],
    queryFn: () => getClientStatus(),
  });
  const balance = useQuery({
    queryKey: ["relay-balance"],
    queryFn: () => getRelayBalance(),
    refetchInterval: 60_000,
  });
  const overview = useQuery({
    queryKey: ["relay-overview"],
    queryFn: () => relayOverview(),
    refetchInterval: 60_000,
  });
  const credentials = useQuery({
    queryKey: ["relay-credentials"],
    queryFn: () => relayCredentials(),
    refetchInterval: 30_000,
  });
  // The account's keys, when signed in. Without a session this stays idle and the
  // page falls back to the keys it can see on disk.
  const accountKeys = useQuery({
    queryKey: ["relay-account-keys"],
    queryFn: () => relayKeysList(),
    enabled: isSignedIn,
    refetchOnWindowFocus: true,
  });

  const refresh = useCallback(async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["aimyria-clients"] }),
      queryClient.invalidateQueries({ queryKey: ["relay-balance"] }),
      queryClient.invalidateQueries({ queryKey: ["relay-overview"] }),
      queryClient.invalidateQueries({ queryKey: ["relay-credentials"] }),
      queryClient.invalidateQueries({ queryKey: ["relay-account-keys"] }),
    ]);
  }, [queryClient]);

  useTauriEvent("deeplink-import", () => {
    void refresh();
  });

  const statuses = clients.data ?? [];
  const matrix: RelayCredentialMatrix | undefined = credentials.data;

  const rows = useMemo(
    () =>
      PRIMARY_AIMYRIA_APPS.map((app) => ({
        app,
        status: statuses.find((item) => item.app === app.id) as
          | ClientStatus
          | undefined,
        binding: matrix?.apps.find((item) => item.app === app.id),
      })),
    [statuses, matrix],
  );

  const boundCount = rows.filter(({ binding }) => binding?.bound).length;
  const mismatched = rows.filter(
    ({ status }) => status !== undefined && !status.stateConsistent,
  );

  const run = useCallback(
    async (app: string, action: () => Promise<unknown>) => {
      setBusyApp(app);
      setError(null);
      try {
        await action();
      } catch (caught) {
        setError(caught instanceof Error ? caught.message : String(caught));
      } finally {
        setBusyApp(null);
        await refresh();
      }
    },
    [refresh],
  );

  // The picker's options: the account's keys when signed in (that is the complete
  // set), plus any key that exists only on this machine.
  //
  // Identity is the fingerprint for local keys and an `acct:<id>` tag for account
  // ones, because the account list never exposes a secret to hash. The tag is what
  // the backend receives; it resolves it back to the key itself.
  const options: Array<{
    value: string;
    /** What the dropdown shows: group, key name, masked secret. */
    label: string;
    /** Second line in the key list: the models that group serves. */
    detail: string | null;
    isRelay: boolean;
    boundApps: number;
  }> = useMemo(() => {
    // A masked secret alone cannot be chosen between. The group says what the key
    // can call and what it costs, and the name is what the user gave it in the
    // console — together they are the two things a person actually recognises.
    const fromAccount = (accountKeys.data ?? []).map((key: RelayKey) => {
      const parts = [
        key.groupName?.trim() || null,
        key.name?.trim() || null,
        key.masked,
      ].filter((part): part is string => Boolean(part));
      return {
        value: `acct:${key.id}`,
        label: parts.join(" · "),
        detail: key.groupModels?.trim() || null,
        isRelay: true,
        boundApps: 0,
      };
    });
    const known = new Set(
      (accountKeys.data ?? []).map((key: RelayKey) => key.masked),
    );
    const localOnly = (matrix?.credentials ?? [])
      .filter((credential) => !known.has(credential.masked))
      .map((credential) => ({
        value: credential.fingerprint,
        label: credential.masked,
        detail: null,
        isRelay: credential.isRelay,
        boundApps: credential.boundApps.length,
      }));
    return [...fromAccount, ...localOnly];
  }, [accountKeys.data, matrix]);

  return (
    <>
      <AppPageHeader
        icon={<AimyriaLogo size={18} />}
        title="接入中心"
        variant="page"
        actions={
          <Button
            variant="quiet"
            size="compact"
            disabled={clients.isFetching || credentials.isFetching}
            onClick={() => void refresh()}
          >
            <RefreshCw
              className={cn(
                "h-3.5 w-3.5",
                (clients.isFetching || credentials.isFetching) && "animate-spin",
              )}
            />
          </Button>
        }
      />

      <div className="min-h-0 flex-1 overflow-y-auto">
        <section className="mx-auto w-full max-w-[720px] px-6 pb-10 pt-8">
          {/* Account row. Signed in: the account name, which is what unlocks the
              full key list. Signed out: the one action that unlocks it. */}
          <div className="mb-4 flex items-center gap-2 text-caption text-fg-3">
            {isSignedIn ? (
              <>
                <span className="inline-block h-1.5 w-1.5 rounded-full bg-success-text" />
                <span className="text-fg-2">{account}</span>
                <span>· {accountKeys.data?.length ?? 0} 把密钥</span>
              </>
            ) : unverified ? (
              <span>正在验证登录…</span>
            ) : (
              <button
                type="button"
                onClick={() => setLoginOpen(true)}
                className="text-fg-2 underline decoration-dotted underline-offset-4 transition-colors hover:text-fg-1"
              >
                登录账户查看全部密钥
              </button>
            )}
          </div>

          {/* Numbers first — what the user checks before anything else. */}
          <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
            <button
              type="button"
              onClick={() => void openExternal(CONSOLE_URL)}
              className="tabular-nums text-[22px] font-semibold leading-none text-fg-1 transition-colors hover:text-fg-2"
            >
              {balance.data?.remaining != null
                ? fmtUsd(balance.data.remaining, 2)
                : "—"}
            </button>
            <span className="text-caption text-fg-3">余额</span>
            <span className="tabular-nums text-caption text-fg-2">
              今日 {fmtCost(overview.data?.usage.todayCost)}
            </span>
            <span className="tabular-nums text-caption text-fg-2">
              本月 {fmtCost(overview.data?.usage.monthCost)}
            </span>
            <span className="tabular-nums text-caption text-fg-2">
              {overview.data?.models.count ?? 0} 模型
            </span>
            <button
              type="button"
              onClick={() => void openExternal(CONSOLE_URL)}
              className="ml-auto text-caption text-fg-3 transition-colors hover:text-fg-1"
            >
              充值 ↗
            </button>
          </div>

          {mismatched.length > 0 ? (
            <div className="mt-4 flex items-start gap-2 rounded-lg border border-border-strong px-3 py-2">
              <CircleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0 text-danger-text" />
              <p className="min-w-0 flex-1 text-caption leading-5 text-fg-2">
                {mismatched.length} 个应用的实际配置与所选不一致，选一个密钥即可重新绑定。
              </p>
            </div>
          ) : null}

          {error ? (
            <p className="mt-3 text-caption text-danger-text">{error}</p>
          ) : null}

          {desktopHint ? (
            <p className="mt-3 text-caption text-fg-2">{desktopHint}</p>
          ) : null}

          {/* Clients × keys. The control IS the binding. */}
          <div className="mt-6 flex items-center justify-between text-caption text-fg-3">
            <span>
              应用 <span className="tabular-nums text-fg-2">{boundCount}</span>/
              {rows.length}
            </span>
            <span>选择密钥即生效</span>
          </div>

          <div className="mt-2 divide-y divide-border overflow-hidden rounded-xl border border-border">
            {rows.map(({ app, status, binding }) => {
              const busy = busyApp === app.id;
              const differs = status !== undefined && !status.stateConsistent;
              const fileBased = app.id === "workbuddy" || app.id === "dsh";
              return (
                <div key={app.id} className="flex items-center gap-3 px-3 py-2.5">
                  <ProviderIcon
                    icon={APP_ICON[app.id]}
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
                    <div className="flex items-center gap-1.5">
                      <span className="truncate text-sm text-fg-1">
                        {app.name}
                      </span>
                      {differs ? (
                        <span className="shrink-0 text-[10px] text-danger-text">
                          不一致
                        </span>
                      ) : null}
                    </div>
                    <div className="truncate text-[11px] text-fg-3">
                      {options.find((option) => option.value === binding?.fingerprint)
                        ?.label ??
                        binding?.masked ??
                        "未绑定"}
                    </div>
                  </div>

                  {/* Picking a key writes it immediately: there is no separate
                      「应用」 button to forget to press. */}
                  <select
                    aria-label={`${app.name} 使用哪个密钥`}
                    value={binding?.fingerprint ?? ""}
                    disabled={busy || options.length === 0}
                    onChange={(event) => {
                      const fingerprint = event.target.value;
                      if (!fingerprint) return;
                      setDesktopHint(null);
                      void run(app.id, async () => {
                        await relayBind(app.id, fingerprint);
                        // Claude Desktop reads its profile once, at startup. Without
                        // this the user has a correct file and an app that cannot
                        // see it — the one step every one-click flow forgets.
                        if (app.id === "claude-desktop") {
                          const outcome = await relayRestartClaudeDesktop();
                          if (!outcome.relaunched && outcome.hint) {
                            setDesktopHint(outcome.hint);
                          }
                        }
                      });
                    }}
                    className="h-7 max-w-[220px] shrink-0 rounded-control border border-border-strong bg-surface px-1.5 text-[11px] text-fg-2 disabled:opacity-50"
                  >
                    <option value="">
                      {options.length === 0 ? "还没有密钥" : "选择密钥…"}
                    </option>
                    {options.map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                        {option.isRelay ? "" : "（第三方）"}
                        {option.boundApps > 1 ? ` · ${option.boundApps} 应用` : ""}
                      </option>
                    ))}
                  </select>

                  {busy ? (
                    <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-fg-3" />
                  ) : null}

                  {fileBased && binding?.bound ? (
                    <div className="flex shrink-0 gap-1">
                      <Button
                        variant="quiet"
                        size="compact"
                        disabled={busy}
                        onClick={() => setPickerApp(app.id)}
                      >
                        模型
                      </Button>
                      <Button
                        variant="quiet"
                        size="compact"
                        disabled={busy}
                        onClick={() =>
                          void run(app.id, () => revertToOfficial(app.id))
                        }
                      >
                        官方
                      </Button>
                    </div>
                  ) : null}
                </div>
              );
            })}
          </div>

          {/* Keys as resources. Masked value, endpoint owner, who uses it. */}
          <div className="mt-6 flex items-center justify-between text-caption text-fg-3">
            <span>
              本机密钥{" "}
              <span className="tabular-nums text-fg-2">{options.length}</span>
            </span>
            <button
              type="button"
              onClick={() => void openExternal(CONSOLE_URL)}
              className="transition-colors hover:text-fg-1"
            >
              管理 ↗
            </button>
          </div>

          {credentials.isLoading ? (
            <p className="mt-3 py-6 text-center text-caption text-fg-3">
              读取中…
            </p>
          ) : options.length === 0 ? (
            // A brand-new install has no keys anywhere. Saying "go to the key page"
            // sends them to a website; the account is what they actually need, and
            // it is one click from here.
            <div className="mt-3 rounded-xl border border-border px-3 py-6 text-center">
              <p className="text-caption text-fg-3">还没有密钥</p>
              {isSignedIn ? (
                <p className="mt-1 text-caption text-fg-3">
                  到这个账户的密钥页创建一把，刷新后即可选用。
                </p>
              ) : (
                <Button
                  variant="outline"
                  size="compact"
                  className="mt-2"
                  onClick={() => setLoginOpen(true)}
                >
                  登录账户获取密钥
                </Button>
              )}
            </div>
          ) : (
            <ul className="mt-2 divide-y divide-border overflow-hidden rounded-xl border border-border">
              {options.map((option) => (
                <li key={option.value} className="px-3 py-2.5">
                  <div className="flex items-center gap-2">
                    <span
                      className={cn(
                        "inline-block h-1.5 w-1.5 shrink-0 rounded-full",
                        option.isRelay ? "bg-success-text" : "bg-border-strong",
                      )}
                      aria-hidden
                    />
                    <span className="min-w-0 flex-1 truncate text-[12px] text-fg-1">
                      {option.label}
                    </span>
                    <span className="shrink-0 text-[11px] text-fg-3">
                      {option.boundApps > 0
                        ? `${option.boundApps} 个应用在用`
                        : "未使用"}
                    </span>
                  </div>
                  {option.detail ? (
                    <p className="mt-0.5 truncate pl-3.5 text-[11px] text-fg-3">
                      {option.detail}
                    </p>
                  ) : null}
                </li>
              ))}
            </ul>
          )}

          <p className="mt-3 text-[11px] text-fg-3">
            与侧边栏互通 · 任一处切换，另一处同步生效
          </p>
        </section>
      </div>

      <RelayLoginDialog open={loginOpen} onOpenChange={setLoginOpen} />

      <RelayModelPicker
        open={pickerApp !== null}
        onOpenChange={(next) => {
          if (!next) setPickerApp(null);
        }}
        appLabel={AIMYRIA_APPS.find((app) => app.id === pickerApp)?.name ?? ""}
        initial={[]}
        busy={busyApp !== null}
        onConfirm={(models) => {
          const target = pickerApp;
          setPickerApp(null);
          if (target) void run(target, () => configureOne(target, { models }));
        }}
      />
    </>
  );
}
