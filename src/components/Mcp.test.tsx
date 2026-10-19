/**
 * Tests for the MCP screen's checks and result reporting.
 *
 * What matters here is honesty about state. A checkbox is a control, but it also
 * *reports* what is written into a client's real config file — so a row that
 * renders as enabled when the database says otherwise is worse than a broken
 * control: it tells the user the tool did something it did not do.
 *
 * `mcp` is mocked so these run in jsdom without a Tauri runtime.
 */

import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { Mcp } from "./Mcp";
import type { McpServer } from "../lib/mcp";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
}));

const invoke = vi.mocked((await import("@tauri-apps/api/core")).invoke);

const server = (over: Partial<McpServer> = {}): McpServer => ({
  id: "ctx7",
  name: "context7",
  server: { command: "npx", args: ["-y", "@upstash/context7-mcp"] },
  apps: { claude: true, codex: false, gemini: false, opencode: false, openclaw: false },
  ...over,
});

beforeEach(() => {
  invoke.mockReset();
});

describe("Mcp", () => {
  it("renders one check per supported client, from the stored state", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_mcp_servers") return [server()];
      return null;
    });

    render(<Mcp />);

    await waitFor(() => expect(screen.getByText("context7")).toBeTruthy());

    // Five boxes, because five clients are supported — not eight, because the
    // other three are not.
    expect(screen.getAllByRole("checkbox")).toHaveLength(5);

    // And the checked one is the one the backend says is on.
    const checked = screen.getAllByRole("checkbox", { checked: true });
    expect(checked).toHaveLength(1);
    expect(checked[0].getAttribute("aria-label")).toContain("Claude Code");
  });

  it("shows the launch command, which is the row's identity", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_mcp_servers") return [server()];
      return null;
    });

    render(<Mcp />);

    // Not the name — the name is the label. What identifies a server is what
    // gets executed.
    await waitFor(() => {
      expect(screen.getByText(/npx -y @upstash\/context7-mcp/)).toBeTruthy();
    });
  });

  it("an empty list is said to be empty, not rendered as zero servers", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_mcp_servers") return [];
      return null;
    });

    render(<Mcp />);

    await waitFor(() => {
      expect(screen.getByText(/还没有 MCP 服务器/)).toBeTruthy();
    });
    // No table at all, rather than a table with an empty footer claiming a count.
    expect(screen.queryAllByRole("checkbox")).toHaveLength(0);
  });

  it("a failed read is reported, not shown as empty", async () => {
    // Same rule as the usage screen: "could not reach the database" and "no
    // servers configured" are different facts and only one of them needs a retry.
    invoke.mockImplementation(async () => {
      throw new Error("db locked");
    });

    render(<Mcp />);

    await waitFor(() => expect(screen.getByText(/读取失败/)).toBeTruthy());
    expect(screen.queryByText(/还没有 MCP 服务器/)).toBeNull();
  });

  it("toggling asks the backend to write, then re-reads what was stored", async () => {
    const calls: string[] = [];
    invoke.mockImplementation(async (cmd: string) => {
      calls.push(cmd);
      if (cmd === "get_mcp_servers") return [server()];
      return null;
    });

    render(<Mcp />);
    await waitFor(() => expect(screen.getByText("context7")).toBeTruthy());

    const boxes = screen.getAllByRole("checkbox");
    // The click writes and then re-reads, so the state settles over several
    // ticks; `act` lets those land before the assertions run.
    await act(async () => {
      boxes[1].click(); // Codex
    });

    await waitFor(() => {
      expect(calls).toContain("set_mcp_enabled");
    });
    // A write must be followed by a read: the UI reports what is on disk, and
    // "optimistic" would mean guessing what the backend did.
    const idxWrite = calls.indexOf("set_mcp_enabled");
    expect(calls.slice(idxWrite + 1)).toContain("get_mcp_servers");
  });
});