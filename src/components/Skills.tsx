/**
 * Skills 管理 — installed skills, and which clients each one is projected into.
 *
 * # The same table as MCP, for a reason
 *
 * A skill is stored once and *projected* into each client's own skills directory.
 * Which clients see it is therefore a per-pair property, not a per-skill one, and
 * a list that grouped by skill would hide it. This is the same table `Mcp.tsx`
 * uses, and deliberately so: the question both screens answer is "what is active
 * where", and two different layouts for one question is worse than one layout
 * used twice.
 *
 * # What a checked box means
 *
 * Exactly what it means on the MCP screen: the skill directory is written into
 * that client's config location. Toggling writes the filesystem immediately, then
 * re-reads — the screen reports what is on disk rather than what was requested.
 *
 * # Why the copy is only partial
 *
 * Upstream's skill screen is ~3,000 lines of React across a 40-dependency stack
 * (radix, react-query, dnd-kit, framer-motion, i18next). This build has three
 * dependencies and a hand-written stylesheet, so those components cannot be
 * copied without adopting that stack wholesale. `lib/skills.ts` *is* copied
 * verbatim — it is a plain `invoke` wrapper — so the API surface here is
 * upstream's, and only the presentation is ours.
 */

import { useCallback, useEffect, useState } from "react";

import { skillsApi, type InstalledSkill, type SkillApps, type UnmanagedSkill } from "../lib/skills";
import { CLIENT_LABELS, MCP_CLIENTS, type McpClient } from "../lib/mcp";
import { Notice } from "./common";

/** Which clients this build configures. Five, not ten — see `lib/mcp.ts`. */
const CLIENTS = MCP_CLIENTS;

/**
 * A freshly imported skill is enabled nowhere.
 *
 * Import means "take this directory into our management", not "activate it" —
 * the user then decides which clients get it, one box at a time. `SkillApps`
 * requires every one of its eight fields, so the four clients this build does not
 * configure are listed as false rather than omitted.
 */
const OFF_FOR_EVERY_CLIENT: SkillApps = {
  claude: false,
  "claude-desktop": false,
  codex: false,
  gemini: false,
  grokbuild: false,
  opencode: false,
  openclaw: false,
  hermes: false,
  pi: false,
  mcode: false,
};

export function Skills() {
  const [skills, setSkills] = useState<InstalledSkill[]>([]);
  const [unmanaged, setUnmanaged] = useState<UnmanagedSkill[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setSkills(await skillsApi.getInstalled());
      setError(null);
    } catch (e) {
      // Same rule as MCP and usage: a failed read is not an empty collection.
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  /** Run an action, report what it actually said, then re-read. */
  const run = useCallback(
    async (key: string, action: () => Promise<string>) => {
      setBusy(key);
      setError(null);
      try {
        setMessage(await action());
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(null);
        await load();
      }
    },
    [load],
  );

  const toggle = useCallback(
    (skill: InstalledSkill, client: McpClient, next: boolean) =>
      run(`${skill.id}:${client}`, async () => {
        await skillsApi.toggleApp(skill.id, client, next);
        return next
          ? `已把 ${skill.name} 写入 ${CLIENT_LABELS[client]}`
          : `已从 ${CLIENT_LABELS[client]} 移除 ${skill.name}`;
      }),
    [run],
  );

  const resync = useCallback(
    () =>
      run("resync", async () => {
        const outcomes = await skillsApi.resyncToApps();
        const failed = outcomes.filter((o) => !o.ok || o.failedSkills.length > 0);
        // Say which clients failed, not just "done" — a silent partial sync is
        // exactly the case where the user needs to know.
        if (failed.length === 0) return "已按开关重新写入所有客户端";
        const names = failed.map((o) => CLIENT_LABELS[o.app as McpClient] ?? o.app);
        return `部分失败：${names.join("、")}`;
      }),
    [run],
  );

  const checkUpdates = useCallback(
    () =>
      run("updates", async () => {
        const result = await skillsApi.checkUpdates();
        if (result.updates.length === 0) return "所有 Skill 已是最新";
        return `有 ${result.updates.length} 个 Skill 可更新`;
      }),
    [run],
  );

  const uninstall = useCallback(
    (skill: InstalledSkill) =>
      run(skill.id, async () => {
        await skillsApi.uninstallUnified(skill.id);
        return `已卸载 ${skill.name}`;
      }),
    [run],
  );

  /** Ask the backend which skills exist in client directories we do not manage. */
  const scanUnmanaged = useCallback(
    () =>
      run("scan", async () => {
        const found = await skillsApi.scanUnmanaged();
        setUnmanaged(found);
        return found.length === 0
          ? "各客户端目录下没有未纳管的 Skill"
          : `发现 ${found.length} 个未纳管的 Skill`;
      }),
    [run],
  );

  const importOne = useCallback(
    (item: UnmanagedSkill) =>
      run(item.path, async () => {
        await skillsApi.importFromApps([
          { directory: item.directory, apps: OFF_FOR_EVERY_CLIENT },
        ]);
        return `已导入 ${item.name}（未在任何客户端启用）`;
      }),
    [run],
  );

  if (loading) return <p className="muted">正在读取 Skills…</p>;

  // A failed read is its own state. Rendering the empty notice under an error
  // would claim both "we could not read" and "you have none", and only one is true.
  if (error && skills.length === 0) {
    return (
      <Notice tone="danger" title="读取失败">
        <p>{error}</p>
      </Notice>
    );
  }

  return (
    <>
      {error ? (
        <Notice tone="danger" title="操作失败">
          <p>{error}</p>
        </Notice>
      ) : null}

      {message ? (
        <Notice tone="brand" title="完成">
          <p>{message}</p>
        </Notice>
      ) : null}

      {unmanaged && unmanaged.length > 0 ? (
        <Notice
          tone="brand"
          title={`发现 ${unmanaged.length} 个未纳管的 Skill`}
          action={
            <button className="link-btn" onClick={() => setUnmanaged(null)}>
              知道了
            </button>
          }
        >
          <p>这些 Skill 已经在某个客户端的目录里，但不在本工具的管理范围内。</p>
          <ul className="unmanaged-list">
            {unmanaged.map((u) => (
              <li key={u.path}>
                <span className="mcp-name">{u.name}</span>
                <span className="muted"> · {u.directory}</span>
                <button
                  className="link-btn"
                  onClick={() => void importOne(u)}
                  disabled={busy === u.path}
                >
                  纳管
                </button>
              </li>
            ))}
          </ul>
        </Notice>
      ) : null}

      <div className="panel">
        <div className="panel-head">
          <span className="panel-title">Skills · {skills.length}</span>
          <div className="seg">
            <button className="seg-btn" onClick={() => void scanUnmanaged()} disabled={busy === "scan"}>
              {busy === "scan" ? "扫描中…" : "扫描"}
            </button>
            <button className="seg-btn" onClick={() => void checkUpdates()} disabled={busy === "check"}>
              {busy === "check" ? "检查中…" : "检查更新"}
            </button>
            <button className="seg-btn" onClick={() => void resync()} disabled={busy === "resync"}>
              {busy === "resync" ? "写入中…" : "重写配置"}
            </button>
          </div>
        </div>

        {skills.length === 0 ? (
          <p className="muted">
            还没有安装任何 Skill。点「扫描」可以从各客户端的目录里找到已经存在、
            但不在本工具管理范围内的 Skill。
          </p>
        ) : (
          <table className="data-table">
            <thead>
              <tr>
                <th>Skill</th>
                {CLIENTS.map((c) => (
                  <th key={c} className="chk">
                    {CLIENT_LABELS[c]}
                  </th>
                ))}
                <th />
              </tr>
            </thead>
            <tbody>
              {skills.map((s) => (
                <tr key={s.id}>
                  <td>
                    <div className="mcp-name">{s.name}</div>
                    <div className="mcp-cmd">{s.directory}</div>
                    {s.description ? <div className="mcp-desc">{s.description}</div> : null}
                  </td>
                  {CLIENTS.map((c) => {
                    const on = s.apps[c] === true;
                    const isBusy = busy === `${s.id}:${c}`;
                    return (
                      <td key={c} className="chk">
                        <button
                          type="button"
                          className="chk-box"
                          role="checkbox"
                          aria-checked={on}
                          aria-label={`${s.name} 在 ${CLIENT_LABELS[c]} 中`}
                          disabled={isBusy}
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
                      onClick={() => void uninstall(s)}
                      disabled={busy === s.id}
                      title="从所有客户端移除"
                    >
                      卸载
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}
