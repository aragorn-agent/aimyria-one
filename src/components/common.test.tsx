/**
 * Tests for the pieces where being wrong would mislead the user.
 *
 * The balance is the case that matters: this tool runs in the background and
 * the number in the tray is the only feedback most users will ever see. If a
 * failed refresh renders as "$0.00", the user is told to top up for a problem
 * that is actually "we could not reach the relay".
 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { BalanceChip, ClientRow, formatUsd, statusState, statusText } from "./common";
import type { ClientStatus } from "../lib/types";

// The UI never talks to Tauri directly, but `invoke` is imported at module load
// through ../lib/api. Stub it so the import resolves under jsdom.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
}));

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

describe("formatUsd", () => {
  it("shows two decimals", () => {
    expect(formatUsd(12)).toBe("$12.00");
    expect(formatUsd(1.5)).toBe("$1.50");
  });

  it("shows a dash rather than zero for a missing amount", () => {
    // The distinction that keeps a failed refresh from looking like $0.00.
    expect(formatUsd(null)).toBe("—");
    expect(formatUsd(0)).toBe("$0.00");
  });

  it("does not print NaN", () => {
    expect(formatUsd(NaN)).toBe("—");
  });
});

describe("BalanceChip", () => {
  it("shows the amount when the relay answered", () => {
    render(<BalanceChip remaining={12.34} low={false} error={null} loading={false} />);
    expect(screen.getByText("$12.34")).toBeTruthy();
  });

  it("says it is temporarily unavailable, not zero, when there is no number", () => {
    render(
      <BalanceChip remaining={null} low={false} error="连接超时" loading={false} />,
    );
    expect(screen.getByText("暂不可用")).toBeTruthy();
    expect(screen.queryByText("$0.00")).toBeNull();
    // The reason is available without cluttering the pill.
    expect(screen.getByText("暂不可用").getAttribute("title")).toBe("连接超时");
  });

  it("marks a low balance", () => {
    const { container } = render(
      <BalanceChip remaining={1.2} low={true} error={null} loading={false} />,
    );
    expect(container.querySelector(".balance")?.getAttribute("data-low")).toBe("true");
  });

  it("does not mark an unknown balance as low", () => {
    const { container } = render(
      <BalanceChip remaining={null} low={false} error="x" loading={false} />,
    );
    expect(container.querySelector(".balance")?.getAttribute("data-low")).toBe("false");
  });

  it("shows a placeholder while loading", () => {
    render(<BalanceChip remaining={null} low={false} error={null} loading />);
    expect(screen.getByText("…")).toBeTruthy();
  });
});

describe("statusText", () => {
  it("confirms a configured client", () => {
    expect(statusText(client({ configured: true }))).toBe("已配置为 Aimyria");
  });

  it("distinguishes not-installed from not-configured", () => {
    // These read very differently to a user deciding what to do next.
    expect(statusText(client({ cliInstalled: false }))).toContain("未检测到");
    // Unbound means no row is selected at all, which is different from a row
    // that exists but has not been written to disk yet.
    expect(statusText(client({ cliInstalled: true, currentProvider: null }))).toBe("未绑定账号");
  });

  it("says a bound-but-unwritten client is one step short", () => {
    expect(statusText(client({ currentProvider: "aimyria" }))).toBe("已绑定账号，尚未写入");
  });

  it("reports an unsupported client instead of pretending", () => {
    expect(statusText(client({ supported: false }))).toBe("当前版本不支持");
  });

  it("confirms a configure that succeeded even without the CLI installed", () => {
    // Regression. `statusText` used to check `cliInstalled` first, so every
    // client whose command-line program is not on this machine kept reading
    // "未检测到命令行工具" — including one the user had just configured by
    // clicking the button. The card looked untouched, so the click looked
    // ineffective.
    //
    // The existing tests all used a fixture with `cliInstalled: true`, which is
    // exactly the case that worked; this is the case that did not.
    const noCliButConfigured = client({ cliInstalled: false, configured: true });
    expect(statusText(noCliButConfigured)).toBe("已配置为 Aimyria");
    expect(statusState(noCliButConfigured)).toBe("ok");
  });

  it("still reports a missing CLI for a client that is not configured", () => {
    // The fix must not swallow the other message: with nothing written yet,
    // "no CLI here" is the useful thing to say.
    expect(statusText(client({ cliInstalled: false, configured: false }))).toContain(
      "未检测到",
    );
    expect(statusState(client({ cliInstalled: false, configured: false }))).toBe("missing");
  });
});

describe("ClientRow", () => {
  it("lets the user configure a bound client", () => {
    const onConfigure = vi.fn();
    render(<ClientRow client={client()} onConfigure={onConfigure} busy={false} disabled={false} />);
    screen.getByRole("button", { name: "配置" }).click();
    expect(onConfigure).toHaveBeenCalledOnce();
  });

  it("disables the button when no account is bound", () => {
    render(
      <ClientRow
        client={client({ currentProvider: null })}
        onConfigure={vi.fn()}
        busy={false}
        disabled={false}
      />,
    );
    const button = screen.getByRole("button", { name: "配置" }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    // And the reason, so the disabled state is not a dead end.
    expect(button.getAttribute("title")).toContain("控制台");
  });

  it("shows progress instead of a clickable label while writing", () => {
    render(<ClientRow client={client()} onConfigure={vi.fn()} busy disabled={false} />);
    expect(screen.getByRole("button", { name: "写入中…" })).toBeTruthy();
  });

  it("marks a configured client in the DOM for styling", () => {
    const { container } = render(
      <ClientRow client={client({ configured: true })} onConfigure={vi.fn()} busy={false} disabled={false} />,
    );
    expect(container.querySelector(".client")?.getAttribute("data-configured")).toBe("true");
  });
});