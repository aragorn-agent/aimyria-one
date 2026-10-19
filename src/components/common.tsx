/**
 * Shared presentational pieces.
 *
 * These carry no state and no IPC — everything they show is passed in, which
 * keeps the screens honest about what is known versus what is assumed.
 */

import type { ClientStatus } from "../lib/types";

/**
 * The balance pill in the title bar.
 *
 * Three states, and they must stay visually distinct: a real number, a number
 * that is too low, and *no number*. The last one matters most — a failed
 * refresh that renders as "$0.00" would tell the user to top up when the
 * problem is that we could not reach the relay.
 */
export function BalanceChip({
  remaining,
  low,
  error,
  loading,
}: {
  remaining: number | null;
  low: boolean;
  error: string | null;
  loading: boolean;
}) {
  const tone = error || remaining === null ? "error" : low ? "low" : "ok";

  return (
    <div className="balance" data-low={String(low)} data-error={String(tone === "error")}>
      <span className="balance-label">余额</span>
      {loading ? (
        <span className="balance-value">…</span>
      ) : tone === "error" ? (
        <span className="balance-value" title={error ?? undefined}>
          暂不可用
        </span>
      ) : (
        <span className="balance-value">{formatUsd(remaining!)}</span>
      )}
    </div>
  );
}

/** USD with two decimals, or an em dash for a missing amount. */
export function formatUsd(value: number | null): string {
  if (value === null || Number.isNaN(value)) return "—";
  return `$${value.toFixed(2)}`;
}

/** A short, plain-language status line for one client. */
export function statusText(client: ClientStatus): string {
  if (!client.supported) return "当前版本不支持";
  // `configured` has to be checked before `cliInstalled`.
  //
  // They are separate facts: we wrote the config file, and separately the
  // command-line program may or may not be installed on this machine. Leading
  // with the CLI check means a client that is configured but not installed here
  // still reads "未检测到命令行工具" — immediately after the user clicked 配置,
  // the card looks exactly as it did before. They conclude the click did
  // nothing and click again.
  if (client.configured) return "已配置为 Aimyria";
  if (!client.cliInstalled) return "未检测到命令行工具，仍可写入配置";
  if (client.currentProvider) return "已绑定账号，尚未写入";
  return "未绑定账号";
}

export function statusState(client: ClientStatus): "ok" | "idle" | "missing" {
  if (!client.supported) return "missing";
  // Same precedence as statusText: a configured client is configured whether or
  // not its CLI is installed, and the dot has to agree with the words.
  if (client.configured) return "ok";
  if (!client.cliInstalled) return "missing";
  return "idle";
}

/** One client row: name, where its config lands, and a button. */
export function ClientRow({
  client,
  onConfigure,
  busy,
  disabled,
}: {
  client: ClientStatus;
  onConfigure: () => void;
  busy: boolean;
  disabled: boolean;
}) {
  const state = statusState(client);
  return (
    <div className="client" data-configured={String(client.configured)}>
      {/* Monogram rather than a logo.
          These are other people's products; drawing their marks would be
          claiming an integration we do not have, and redrawing them badly would
          be worse than not. A letter is enough to scan a list by, and it keeps
          the row visually anchored the way an icon would. */}
      <span className="client-mark" data-state={state} aria-hidden="true">
        {client.label.replace(/[^A-Za-z]/g, "").charAt(0) || "?"}
      </span>
      <div className="client-main">
        <div className="client-name">
          {client.label}
          {state === "ok" ? <span className="client-tag">已配置</span> : null}
        </div>
        <div className="client-path">
          {client.livePath ?? "—"} · {statusText(client)}
        </div>
      </div>
      <button
        className="button"
        onClick={onConfigure}
        disabled={disabled || busy || !client.currentProvider}
        title={
          client.currentProvider
            ? state === "ok"
              ? `重新写入 ${client.label} 的配置`
              : `写入 ${client.label} 的配置`
            : "请先从 Aimyria 控制台获取配置"
        }
      >
        {busy ? "写入中…" : state === "ok" ? "重新配置" : "配置"}
      </button>
    </div>
  );
}

/**
 * A boxed message.
 *
 * `tone` drives the dot colour only — never the whole block. A warning that
 * repaints the panel reads as an error dialog, and this tool is background
 * software: it should inform, not alarm.
 */
export function Notice({
  tone = "brand",
  title,
  children,
  action,
}: {
  tone?: "brand" | "warn" | "danger";
  title: string;
  children?: React.ReactNode;
  action?: React.ReactNode;
}) {
  return (
    <div className="notice" data-tone={tone} role={tone === "danger" ? "alert" : undefined}>
      <span className="notice-mark" aria-hidden="true" />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="notice-title">{title}</div>
        {children ? <div className="notice-body">{children}</div> : null}
        {action ? <div style={{ marginTop: 10 }}>{action}</div> : null}
      </div>
    </div>
  );
}

/** A settings row with an optional right-hand control. */
export function Row({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="row">
      <div className="row-main">
        <div className="row-title">{title}</div>
        {hint ? <div className="row-hint">{hint}</div> : null}
      </div>
      {children}
    </div>
  );
}