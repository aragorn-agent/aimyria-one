/**
 * MCP 管理 — servers, and which clients each is enabled in.
 *
 * Upstream's screen is a table: server name on the left, one checkbox per client
 * across the top, and a row of actions. That layout is worth keeping because it
 * answers the question this screen exists for — "what is active where" — in one
 * glance, without opening anything.
 *
 * # What the checks mean
 *
 * A checked box says the server is written into that client's config file, which
 * is the only thing that determines whether the CLI sees it. It is not a
 * preference: toggling one writes that client's file immediately.
 *
 * # Reuse over re-implementation
 *
 * This component is the one place in the app that manages a collection, so it
 * carries the pieces the rest of the UI needs too: the empty state, the
 * in-progress state, and a result banner that says what actually happened. Those
 * were lifted from Usage rather than written again, which is why they behave the
 * same way — an empty collection is said to be empty, and a failed write is said
 * to have failed.
 */

import { useCallback, useEffect, useState } from "react";

import {
  CLIENT_LABELS,
  MCP_CLIENTS,
  mcp,
  type McpClient,
  type McpServer,
} from "../lib/mcp";
import { Notice } from "./common";

export function Mcp() {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setServers(await mcp.list());
      setError(null);
    } catch (e) {
      // Same rule as the usage screen: a failed read is not an empty list.
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  /** Toggle one server in one client. Optimistic, then re-read from disk. */
  const toggle = useCallback(async (server: McpServer, client: McpClient, next: boolean) => {
    setBusyId(`${server.id}:${client}`);
    try {
      await mcp.setEnabled(server.id, client, next);
      setMessage(next ? `已加入 ${CLIENT_LABELS[client]}` : `已从 ${CLIENT_LABELS[client]} 移除`);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  }, [load]);

  const importFromApps = useCallback(async () => {
    setBusyId("import");
    try {
      const n = await mcp.importFromApps();
      setMessage(n > 0 ? `从客户端导入了 ${n} 个服务` : "没有发现新的可导入服务");
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  }, [load]);

  const resync = useCallback(async () => {
    setBusyId("resync");
    try {
      await mcp.resync();
      setMessage("已按数据库重写所有客户端配置");
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  }, [load]);

  const remove = useCallback(async (server: McpServer) => {
    setBusyId(server.id);
    try {
      await mcp.remove(server.id);
      setMessage(`已删除 ${server.name}`);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  }, [load]);

  if (loading) return <p className="muted">正在读取 MCP 服务器…</p>;

  // A failed read is its own state, not an empty one. Rendering the empty
  // notice underneath the error would tell the user both "we could not read"
  // and "you have no servers", which is contradictory and only one of them is
  // true.
  if (error) {
    return (
      <Notice tone="danger" title="读取失败">
        <p>{error}</p>
      </Notice>
    );
  }

  return (
    <>
      {message ? (
        <Notice tone="brand" title="完成">
          <p>{message}</p>
        </Notice>
      ) : null}

      {servers.length === 0 ? (
        <Notice
          tone="brand"
          title="还没有 MCP 服务器"
          action={
            <button
              className="button button-primary"
              onClick={() => void importFromApps()}
              disabled={busyId === "import"}
            >
              {busyId === "import" ? "正在导入…" : "从各客户端导入"}
            </button>
          }
        >
          <p>
            如果已经在 Claude Code、Codex 等工具里手工配过 MCP 服务器，
            可以从它们的配置文件里直接读进来，不必重新填一遍。
          </p>
        </Notice>
      ) : (
        <div className="panel">
          <div className="panel-head">
            <span className="panel-title">MCP 服务器 · {servers.length}</span>
            <div className="seg">
              <button
                className="seg-btn"
                onClick={() => void importFromApps()}
                disabled={busyId === "import"}
                title="从各客户端配置文件中读取已有的 MCP 服务器"
              >
                导入
              </button>
              <button
                className="seg-btn"
                onClick={() => void resync()}
                disabled={busyId === "resync"}
                title="按数据库重写所有客户端配置文件"
              >
                重写配置
              </button>
            </div>
          </div>

          <table className="data-table">
            <thead>
              <tr>
                <th>服务器</th>
                {MCP_CLIENTS.map((c) => (
                  <th key={c} className="chk" title={CLIENT_LABELS[c]}>
                    {CLIENT_LABELS[c]}
                  </th>
                ))}
                <th />
              </tr>
            </thead>
            <tbody>
              {servers.map((s) => (
                <tr key={s.id}>
                  <td>
                    <div className="mcp-name">{s.name}</div>
                    <div className="mcp-cmd">{commandLine(s.server)}</div>
                    {s.description ? <div className="mcp-desc">{s.description}</div> : null}
                  </td>
                  {MCP_CLIENTS.map((c) => {
                    // `McpApps` has one field per client this build can see, so a
                    // missing field would be a real bug rather than "off". Indexing
                    // it with a union is what broke: the two sides have to agree.
                    const on = s.apps[c];
                    const busy = busyId === `${s.id}:${c}`;
                    return (
                      <td key={c} className="chk">
                        <button
                          type="button"
                          className="chk-box"
                          role="checkbox"
                          aria-checked={on}
                          aria-label={`${s.name} 在 ${CLIENT_LABELS[c]} 中`}
                          disabled={busy}
                          onClick={() => void toggle(s, c, !on)}
                          title={on ? "点击移除" : "点击写入"}
                        >
                          {on ? "✓" : ""}
                        </button>
                      </td>
                    );
                  })}
                  <td className="act">
                    <button
                      className="link-btn"
                      onClick={() => void remove(s)}
                      disabled={busyId === s.id}
                      title="从数据库删除"
                    >
                      删除
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}

/**
 * `npx -y some-package args` from the stored server spec.
 *
 * The spec is the JSON the client expects verbatim, so this is read-only: if it
 * is not the usual command-and-args shape, say so rather than guessing.
 */
function commandLine(server: Record<string, unknown>): string {
  const cmd = typeof server.command === "string" ? server.command : null;
  if (!cmd) return "（未提供启动命令）";
  const args = Array.isArray(server.args) ? server.args.map(String) : [];
  return [cmd, ...args].join(" ");
}