/**
 * Home — the only screen that matters.
 *
 * One relay, five clients, one button. Everything else the tool can do is
 * either in Settings or unnecessary, which is the whole point: a user who
 * installed this should not have to read anything to use it.
 */

import { CONSOLE_URL, openExternal } from "../lib/api";
import type { ClientStatus, ConfigureReport, RelayBalance } from "../lib/types";
import { BalanceChip, ClientRow, Notice, formatUsd } from "./common";

export function Home({
  clients,
  balance,
  balanceLoading,
  busyApp,
  busyAll,
  report,
  onConfigureAll,
  onConfigureOne,
}: {
  clients: ClientStatus[];
  balance: RelayBalance;
  balanceLoading: boolean;
  /** App currently being written, or null. */
  busyApp: string | null;
  busyAll: boolean;
  /** Result of the last run, so a failure stays on screen until dismissed. */
  report: ConfigureReport | null;
  onConfigureAll: () => void;
  onConfigureOne: (app: string) => void;
}) {
  const bound = clients.some((c) => c.currentProvider);
  const ready = clients.filter((c) => c.currentProvider);
  const configuredCount = clients.filter((c) => c.configured).length;
  const writing = busyAll || busyApp !== null;

  return (
    <>
      <BalanceChip
        remaining={balance.remaining}
        low={balance.low}
        error={balance.error}
        loading={balanceLoading}
      />

      <div style={{ height: 18 }} />

      {balance.low ? (
        <Notice tone="warn" title="余额偏低">
          <p>
            当前余额 {formatUsd(balance.remaining)}，低于 $2.00。工具仍可继续使用，
            但请求可能被中转站拒绝。
          </p>
        </Notice>
      ) : null}

      {!bound ? (
        <Notice
          tone="brand"
          title="还没有绑定 Aimyria 账号"
          action={
            <button
              className="button button-primary"
              onClick={() => void openExternal(CONSOLE_URL)}
            >
              前往控制台获取配置
            </button>
          }
        >
          <p>
            在控制台登录后点击「一键配置」，浏览器会通过 <code>aimyria://</code>{" "}
            把账号交到这个工具，之后所有配置都由它完成。
          </p>
        </Notice>
      ) : null}

      {report && report.failed.length === 0 && report.warnings.length === 0 ? (
        <Notice tone="brand" title="配置完成">
          <p>
            {report.configured.length === clients.length
              ? `${clients.length} 个客户端都已指向 Aimyria。`
              : `已写入 ${report.configured.length} 个客户端：${report.configured.join("、")}。`}
            之后直接用命令行工具即可，不需要再打开这个工具。
          </p>
        </Notice>
      ) : null}

      {report && report.failed.length > 0 ? (
        <Notice tone="danger" title="有客户端未能写入">
          <ul style={{ margin: "4px 0 0", paddingLeft: 18 }}>
            {report.failed.map(([app, reason]) => (
              <li key={app}>
                <strong>{app}</strong>：{reason}
              </li>
            ))}
          </ul>
        </Notice>
      ) : null}

      {report && report.warnings.length > 0 ? (
        <Notice tone="warn" title="写入完成，但有提示">
          <ul style={{ margin: "4px 0 0", paddingLeft: 18 }}>
            {report.warnings.map((w, i) => (
              <li key={i}>{w}</li>
            ))}
          </ul>
        </Notice>
      ) : null}

      <h2 className="section-heading">
        客户端{bound ? ` · ${configuredCount}/${clients.length} 已配置` : ""}
      </h2>

      <div className="clients">
        {clients.map((client) => (
          <ClientRow
            key={client.app}
            client={client}
            busy={busyApp === client.app}
            disabled={!bound || writing}
            onConfigure={() => onConfigureOne(client.app)}
          />
        ))}
      </div>

      <div className="button-row">
        <button
          className="button button-primary"
          onClick={onConfigureAll}
          disabled={!bound || writing || ready.length === 0}
        >
          {busyAll ? "正在写入…" : "一键配置全部"}
        </button>
        <button
          className="button"
          onClick={() => void openExternal(CONSOLE_URL)}
          disabled={writing}
        >
          管理账号
        </button>
      </div>

      {!bound ? (
        <p style={{ marginTop: 14, fontSize: 12, color: "var(--ink-3)" }}>
          绑定账号后，「配置」按钮才会生效——这是为了避免把一个还没有 Key 的配置写进客户端。
        </p>
      ) : null}
    </>
  );
}