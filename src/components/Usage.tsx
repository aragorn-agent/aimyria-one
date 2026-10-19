/**
 * 用量统计 — the dashboard.
 *
 * This screen exists to answer three questions, in order: how much have I
 * spent, how have I been spending it, and what happened when something went
 * wrong. Everything else is subordinate.
 *
 * # The empty state is the important part
 *
 * With direct connections this tool does not sit on the request path: the CLI
 * talks to the relay itself, so per-request logs are not something it can see.
 * An empty table is therefore the *expected* state, not a bug.
 *
 * A chart drawn from nothing would be worse than no chart — it would look like
 * a real measurement of zero activity rather than an absence of data. So when
 * there is nothing recorded, this says which of the two reasons applies.
 */

import { useCallback, useEffect, useState } from "react";

import { usage } from "../lib/usage";
import type { DailyStats, ModelStats, ProviderStats, UsageSummary } from "../lib/types";
import { formatUsd, Notice } from "./common";

type Range = 1 | 7 | 30;

export function Usage({
  balance,
  onOpenConsole,
}: {
  balance: number | null;
  onOpenConsole: () => void;
}) {
  const [summary, setSummary] = useState<UsageSummary | null>(null);
  const [trends, setTrends] = useState<DailyStats[]>([]);
  const [models, setModels] = useState<ModelStats[]>([]);
  const [providers, setProviders] = useState<ProviderStats[]>([]);
  const [days, setDays] = useState<Range>(7);
  const [metric, setMetric] = useState<"tokens" | "cost">("tokens");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [s, t, m, p] = await Promise.all([
        usage.summary(),
        usage.trends(days),
        usage.models(),
        usage.providers(),
      ]);
      setSummary(s);
      setTrends(t);
      setModels(m);
      setProviders(p);
      setError(null);
    } catch (e) {
      // A failed query is not a zero query. Saying "0 requests" when the call
      // errored is the same lie as drawing an empty chart.
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [days]);

  useEffect(() => {
    void load();
  }, [load]);

  const hasData = (summary?.totalRequests ?? 0) > 0;

  return (
    <>
      <div className="stat-row">
        <Stat
          label="余额"
          value={balance === null ? "—" : formatUsd(balance)}
          hint={balance === null ? "无法读取" : "Aimyria 账户"}
        />
        <Stat
          label="总请求数"
          value={summary ? summary.totalRequests.toLocaleString() : "—"}
          hint={summary ? `成功率 ${(summary.successRate * 100).toFixed(0)}%` : ""}
        />
        <Stat
          label="真实消耗 Tokens"
          value={summary ? summary.realTotalTokens.toLocaleString() : "—"}
          hint={summary ? `输入 ${fmtCompact(summary.totalInputTokens)} · 输出 ${fmtCompact(summary.totalOutputTokens)}` : ""}
        />
        <Stat
          label="缓存命中率"
          value={summary ? `${(summary.cacheHitRate * 100).toFixed(1)}%` : "—"}
          hint={summary ? `读取 ${fmtCompact(summary.totalCacheReadTokens)}` : ""}
        />
      </div>

      <div style={{ height: 18 }} />

      {error ? (
        <Notice tone="danger" title="用量数据读取失败">
          <p>{error}</p>
        </Notice>
      ) : null}

      {balance !== null && balance < 2 ? (
        <Notice tone="warn" title="余额偏低">
          <p>
            当前余额 {formatUsd(balance)}，低于 $2.00。工具仍可继续使用，但请求可能被中转站拒绝。
          </p>
        </Notice>
      ) : null}

      {hasData ? (
        <>
          <div className="panel">
            <div className="panel-head">
              <span className="panel-title">使用趋势</span>
              <div className="seg">
                {(["tokens", "cost"] as const).map((m) => (
                  <button
                    key={m}
                    className="seg-btn"
                    data-on={String(metric === m)}
                    onClick={() => setMetric(m)}
                  >
                    {m === "tokens" ? "Tokens" : "成本"}
                  </button>
                ))}
              </div>
              <div className="seg" style={{ marginLeft: 8 }}>
                {([1, 7, 30] as const).map((d) => (
                  <button
                    key={d}
                    className="seg-btn"
                    data-on={String(days === d)}
                    onClick={() => setDays(d)}
                  >
                    {d === 1 ? "当天" : `${d} 天`}
                  </button>
                ))}
              </div>
            </div>
            <Sparkline data={trends} metric={metric} />
          </div>

          <div style={{ height: 14 }} />

          <div className="panel">
            <div className="panel-head">
              <span className="panel-title">按模型</span>
            </div>
            <table className="data-table">
              <thead>
                <tr>
                  <th>模型</th>
                  <th className="num">请求</th>
                  <th className="num">Tokens</th>
                  <th className="num">成本</th>
                  <th className="num">成功率</th>
                </tr>
              </thead>
              <tbody>
                {models.slice(0, 12).map((m) => (
                  <tr key={m.model}>
                    <td>{m.model}</td>
                    <td className="num">{m.requestCount.toLocaleString()}</td>
                    <td className="num">{fmtCompact(m.totalTokens)}</td>
                    <td className="num">{formatUsd(Number(m.totalCost))}</td>
                    <td className="num">{(m.successRate * 100).toFixed(0)}%</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      ) : (
        <Notice
          tone="brand"
          title="还没有可展示的用量记录"
          action={
            <button className="button button-primary" onClick={onOpenConsole}>
              查看账户明细
            </button>
          }
        >
          <p>
            本工具让命令行直接连接 Aimyria，请求不经过它，因此看不到逐条请求日志。
            余额和花费以 Aimyria 控制台的数据为准；这里会显示工具自身能记录的部分。
          </p>
        </Notice>
      )}

      {providers.length > 0 ? (
        <>
          <div style={{ height: 14 }} />
          <div className="panel">
            <div className="panel-head">
              <span className="panel-title">按客户端</span>
            </div>
            <table className="data-table">
              <thead>
                <tr>
                  <th>客户端</th>
                  <th className="num">请求</th>
                  <th className="num">Tokens</th>
                  <th className="num">成本</th>
                  <th className="num">平均延迟</th>
                </tr>
              </thead>
              <tbody>
                {providers.map((p) => (
                  <tr key={`${p.providerId}-${p.appType}`}>
                    <td>
                      {p.providerName}
                      <span className="muted"> · {p.appType}</span>
                    </td>
                    <td className="num">{p.requestCount.toLocaleString()}</td>
                    <td className="num">{fmtCompact(p.totalTokens)}</td>
                    <td className="num">{formatUsd(Number(p.totalCost))}</td>
                    <td className="num">{p.avgLatencyMs} ms</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      ) : null}

      {loading ? <p className="muted" style={{ marginTop: 12 }}>正在读取…</p> : null}
    </>
  );
}

function Stat({
  label,
  value,
  hint,
}: {
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div className="stat">
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
      {hint ? <div className="stat-hint">{hint}</div> : null}
    </div>
  );
}

/**
 * A dependency-free bar chart.
 *
 * Not a charting library: this is fourteen lines of arithmetic, and shipping a
 * graph library for it would add a dependency for something the palette and the
 * data shape do not require.
 */
function Sparkline({
  data,
  metric,
}: {
  data: DailyStats[];
  metric: "tokens" | "cost";
}) {
  const values = data.map((d) =>
    metric === "tokens" ? d.totalTokens : Number(d.totalCost),
  );
  const max = Math.max(1, ...values);

  if (values.length === 0) {
    return <p className="muted">所选区间内没有记录。</p>;
  }

  return (
    <div className="chart">
      {data.map((d, i) => (
        <div className="chart-col" key={d.date} title={`${d.date} · ${values[i]}`}>
          <div
            className="chart-bar"
            style={{ height: `${Math.max(2, (values[i] / max) * 100)}%` }}
          />
          <span className="chart-label">{d.date.slice(5)}</span>
        </div>
      ))}
    </div>
  );
}

/** 1234567 → 1.2M. Keeps the column narrow without losing the magnitude. */
function fmtCompact(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}K`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}