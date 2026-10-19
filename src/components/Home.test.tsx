/**
 * Home — the one screen, and the states it has to get right.
 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Home } from "./Home";
import type { ClientStatus, RelayBalance } from "../lib/types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
}));

const NO_BALANCE: RelayBalance = {
  remaining: null,
  used: null,
  low: false,
  error: null,
};

function client(over: Partial<ClientStatus> = {}): ClientStatus {
  return {
    app: "claude",
    label: "Claude Code",
    supported: true,
    cliInstalled: true,
    livePath: "C:/Users/a/.claude/settings.json",
    configured: false,
    currentProvider: "aimyria",
    ...over,
  };
}

function renderHome(props: Partial<React.ComponentProps<typeof Home>> = {}) {
  const defaults: React.ComponentProps<typeof Home> = {
    clients: [client(), client({ app: "codex", label: "Codex", configured: true })],
    balance: NO_BALANCE,
    balanceLoading: false,
    busyApp: null,
    busyAll: false,
    report: null,
    onConfigureAll: vi.fn(),
    onConfigureOne: vi.fn(),
  };
  return render(<Home {...defaults} {...props} />);
}

describe("Home", () => {
  it("offers the console when no account is bound, and disables writing", () => {
    renderHome({ clients: [client({ currentProvider: null })] });

    expect(screen.getByText("还没有绑定 Aimyria 账号")).toBeTruthy();
    expect(
      (screen.getByRole("button", { name: "一键配置全部" }) as HTMLButtonElement).disabled,
    ).toBe(true);
    expect(
      (screen.getByRole("button", { name: "配置" }) as HTMLButtonElement).disabled,
    ).toBe(true);
  });

  it("enables 一键配置全部 once an account is bound", () => {
    renderHome();
    expect(
      (screen.getByRole("button", { name: "一键配置全部" }) as HTMLButtonElement).disabled,
    ).toBe(false);
  });

  it("warns when the balance is low", () => {
    renderHome({ balance: { ...NO_BALANCE, remaining: 1.1, low: true } });
    expect(screen.getByText("余额偏低")).toBeTruthy();
    // The amount appears twice — in the title-bar pill and in the notice — so
    // assert on the notice's own text rather than a bare amount match.
    expect(screen.getByText(/低于 \$2\.00/)).toBeTruthy();
  });

  it("does not warn when the balance is healthy", () => {
    renderHome({ balance: { ...NO_BALANCE, remaining: 40, low: false } });
    expect(screen.queryByText("余额偏低")).toBeNull();
  });

  it("surfaces per-client write failures instead of swallowing them", () => {
    renderHome({
      report: {
        configured: [],
        failed: [["codex", "config.toml 不是合法 TOML"]],
        warnings: [],
      },
    });
    expect(screen.getByText("有客户端未能写入")).toBeTruthy();
    expect(screen.getByText(/config.toml 不是合法 TOML/)).toBeTruthy();
  });

  it("shows a failure alongside the clients that did succeed", () => {
    renderHome({
      report: {
        configured: ["claude"],
        failed: [["codex", "写入失败"]],
        warnings: [],
      },
    });
    expect(screen.getByText("有客户端未能写入")).toBeTruthy();
    expect(screen.getByText("Claude Code")).toBeTruthy();
  });

  it("reports warnings separately from failures", () => {
    renderHome({
      report: { configured: ["claude"], failed: [], warnings: ["已覆盖你手写的 env 段"] },
    });
    expect(screen.queryByText("有客户端未能写入")).toBeNull();
    expect(screen.getByText("写入完成，但有提示")).toBeTruthy();
  });

  it("counts configured clients in the heading", () => {
    renderHome();
    expect(screen.getByText(/1\/2 已配置/)).toBeTruthy();
  });

  it("does not show the count while nothing is bound", () => {
    renderHome({ clients: [client({ currentProvider: null })] });
    expect(screen.queryByText(/已配置/)).toBeNull();
  });

  it("blocks a second write while one is running", () => {
    renderHome({ busyAll: true });
    expect(
      (screen.getByRole("button", { name: "正在写入…" }) as HTMLButtonElement).disabled,
    ).toBe(true);
  });
});