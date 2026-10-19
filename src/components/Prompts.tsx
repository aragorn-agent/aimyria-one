/**
 * 提示词管理 — the prompt library, one client at a time.
 *
 * # Why this screen is per-client and MCP is not
 *
 * An MCP server can be written into five clients independently, so one row with
 * five columns answers the whole question at once. A prompt library cannot:
 * `get_prompts(app)` returns the prompts *for that client*, and the backend stores
 * one library per app. Showing all five in one list would need a union of five
 * unrelated libraries with no way to tell which client each row belongs to — so
 * the client is chosen first, and the list below shows only that client's.
 *
 * # What enabling a prompt does
 *
 * `enable_prompt` makes that prompt the one written into the client's prompt file
 * (`CLAUDE.md`, `AGENTS.md`, …). It is exclusive: the backend promotes one and
 * demotes the rest, so this screen shows at most one enabled row at a time rather
 * than a column of independent toggles.
 */

import { useCallback, useEffect, useState } from "react";

import { promptsApi, type Prompt } from "../lib/prompts";
import { CLIENT_LABELS, MCP_CLIENTS, type McpClient } from "../lib/mcp";
import { Notice } from "./common";

export function Prompts() {
  const [client, setClient] = useState<McpClient>("claude");
  const [prompts, setPrompts] = useState<Record<string, Prompt>>({});
  const [location, setLocation] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [draft, setDraft] = useState("");

  const load = useCallback(async (app: McpClient) => {
    setLoading(true);
    try {
      const [list, where] = await Promise.all([
        promptsApi.getPrompts(app),
        promptsApi.getFileLocation(app).catch(() => null),
      ]);
      setPrompts(list);
      setLocation(where?.displayPath ?? null);
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load(client);
  }, [client, load]);

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
        await load(client);
      }
    },
    [client, load],
  );

  const save = useCallback(
    () =>
      run("save", async () => {
        const body = draft.trim();
        if (!body) return "内容为空，没有保存";
        const id = body.slice(0, 24).replace(/\s+/g, "-") || "prompt";
        await promptsApi.upsertPrompt(client, id, {
          id,
          name: body.slice(0, 24),
          content: body,
          enabled: false,
        });
        setDraft("");
        return `已保存到 ${CLIENT_LABELS[client]}`;
      }),
    [client, draft, run],
  );

  const enable = useCallback(
    (p: Prompt) =>
      run(p.id, async () => {
        await promptsApi.enablePrompt(client, p.id);
        return `已启用 ${p.name}，其余提示词已停用`;
      }),
    [client, run],
  );

  const remove = useCallback(
    (p: Prompt) =>
      run(p.id, async () => {
        await promptsApi.deletePrompt(client, p.id);
        return `已删除 ${p.name}`;
      }),
    [client, run],
  );

  const importFromFile = useCallback(
    () =>
      run("import", async () => {
        const name = await promptsApi.importFromFile(client);
        return name ? `已从文件导入 ${name}` : "没有导入任何提示词";
      }),
    [client, run],
  );

  const list = Object.values(prompts).sort((a, b) => a.name.localeCompare(b.name));

  // A failed read is its own state, not an empty one. Rendering "no prompts yet"
  // underneath the error would tell the user both "we could not read" and "you
  // have none", and only one of those is true. Same rule as MCP and Skills.
  if (error && Object.keys(prompts).length === 0) {
    return (
      <>
        <div className="seg client-seg" role="tablist" aria-label="选择客户端">
          {MCP_CLIENTS.map((c) => (
            <button
              key={c}
              className="seg-btn"
              role="tab"
              aria-selected={client === c}
              onClick={() => setClient(c)}
            >
              {CLIENT_LABELS[c]}
            </button>
          ))}
        </div>
        <Notice tone="danger" title="读取失败">
          <p>{error}</p>
        </Notice>
      </>
    );
  }

  return (
    <>
      <div className="seg client-seg" role="tablist" aria-label="选择客户端">
        {MCP_CLIENTS.map((c) => (
          <button
            key={c}
            className="seg-btn"
            role="tab"
            aria-selected={client === c}
            onClick={() => setClient(c)}
          >
            {CLIENT_LABELS[c]}
          </button>
        ))}
      </div>

      {error ? (
        <Notice tone="danger" title="读取失败">
          <p>{error}</p>
        </Notice>
      ) : null}

      {message ? (
        <Notice tone="brand" title="完成">
          <p>{message}</p>
        </Notice>
      ) : null}

      <div className="panel">
        <div className="panel-head">
          <span className="panel-title">
            {CLIENT_LABELS[client]} · {list.length}
          </span>
          <div className="seg">
            <button
              className="seg-btn"
              onClick={() => void importFromFile()}
              disabled={busy === "import"}
              title="读取该客户端当前使用的提示词文件"
            >
              从文件导入
            </button>
          </div>
        </div>

        {location ? <p className="muted path-hint">写入目标：{location}</p> : null}

        {loading ? (
          <p className="muted">正在读取…</p>
        ) : list.length === 0 ? (
          <p className="muted">
            {CLIENT_LABELS[client]} 还没有提示词。可以在下面直接写一条，
            或点「从文件导入」把它现在用的提示词文件读进来。
          </p>
        ) : (
          <ul className="prompt-list">
            {list.map((p) => (
              <li key={p.id} className="prompt-item">
                <div className="prompt-head">
                  <span className="mcp-name">{p.name}</span>
                  {p.enabled ? <span className="badge">使用中</span> : null}
                </div>
                <p className="prompt-body">{p.content}</p>
                <div className="prompt-actions">
                  {!p.enabled ? (
                    <button
                      className="link-btn"
                      onClick={() => void enable(p)}
                      disabled={busy === p.id}
                    >
                      启用
                    </button>
                  ) : null}
                  <button
                    className="link-btn"
                    onClick={() => void remove(p)}
                    disabled={busy === p.id}
                  >
                    删除
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}

        <div className="prompt-editor">
          <textarea
            className="prompt-input"
            placeholder={`给 ${CLIENT_LABELS[client]} 写一条新提示词…`}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            rows={4}
          />
          <button
            className="button button-primary"
            onClick={() => void save()}
            disabled={busy === "save" || draft.trim() === ""}
          >
            {busy === "save" ? "保存中…" : "保存"}
          </button>
        </div>
      </div>
    </>
  );
}
