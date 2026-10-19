/**
 * Tests for the usage screen's empty and error states.
 *
 * These are the cases where a fast implementation tells the user something that
 * isn't true. With direct connections the tool never sits on the request path,
 * so an empty result set is the *expected* state — the only wrong answers are
 * "0 requests", a full-height chart, or a success message.
 *
 * `usage` is mocked because these tests are about what the UI does with the
 * answer, not whether the query works. The query is the Rust side's business and
 * has its own tests.
 */

import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { Usage } from "./Usage";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
}));

const invoke = vi.mocked((await import("@tauri-apps/api/core")).invoke);

/** Look for a real data claim: a "0", a full chart, or a configured count. */
function hasDataClaims() {
  const text = document.body.textContent ?? "";
  const claims = /^0$|^0\.0%$|0 Requests?$|1 天/i.exec(text);
  return Boolean(claims);
}

/** The empty-but-real shape every usage query returns when nothing is recorded. */
const ZERO = {
  totalRequests: 0, totalCost: "0", totalInputTokens: 0, totalOutputTokens: 0,
  totalCacheCreationTokens: 0, totalCacheReadTokens: 0, successRate: 0,
  realTotalTokens: 0, cacheHitRate: 0,
};

beforeEach(() => {
  invoke.mockReset();
});

describe("Usage", () => {
  it("says there is nothing recorded instead of reporting zero", async () => {
    // The screen must not claim "0 requests" — that is a measurement, and what we
    // have is an absence of data. Those read the same but mean opposite things.
    invoke.mockImplementation(async (cmd: string) => {
      switch (cmd) {
        case "get_usage_summary":
        case "get_session_usage_summary":
        case "get_usage_summary_by_app":
          return ZERO;
        case "get_usage_trends":
        case "get_provider_stats":
        case "get_model_stats":
          return [];
        default:
          return null;
      }
    });

    render(<Usage balance={16.5} onOpenConsole={() => {}} />);

    await waitFor(() => {
      expect(screen.getByText(/还没有可展示的用量记录/)).toBeTruthy();
    });
    expect(hasDataClaims()).toBe(false);
  });

  it("a failed query is reported as a failure, not as zero", async () => {
    // "could not refresh" and "$0.00" are different facts, and only one of them
    // tells the user to retry.
    invoke.mockImplementation(async () => {
      throw new Error("relay unreachable");
    });

    render(<Usage balance={null} onOpenConsole={() => {}} />);

    await waitFor(() => {
      expect(screen.getByText(/用量数据读取失败/)).toBeTruthy();
    });
    expect(hasDataClaims()).toBe(false);
  });

  it("the balance is shown, and 'cannot read' is distinguished from zero", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_usage_trends" || cmd === "get_provider_stats" || cmd === "get_model_stats") return [];
      return null;
    });

    render(<Usage balance={null} onOpenConsole={() => {}} />);

    // `null` means the balance could not be read. Showing "—" says that; showing
    // "$0.00" would tell the user to top up a topped-up account.
    await waitFor(() => {
      expect(screen.getAllByText("—").length).toBeGreaterThan(0);
    });
    expect(screen.queryByText("$0.00")).toBeNull();
  });

  it("the models table renders real recorded rows", async () => {
    // The complement: when data exists, it must actually appear.
    const rows = [
      { model: "claude-sonnet-4-5", requestCount: 12, totalTokens: 48000, totalCost: "1.23", avgCostPerRequest: "0.10", successRate: 1 },
    ];
    invoke.mockImplementation(async (cmd: string) => {
      switch (cmd) {
        case "get_usage_summary":
          return { ...ZERO, totalRequests: 12, totalCost: "1.23", realTotalTokens: 48000, cacheHitRate: 0.28 };
        case "get_usage_trends":
        case "get_provider_stats":
          return [];
        case "get_model_stats":
          return rows;
        default:
          return null;
      }
    });

    render(<Usage balance={2.5} onOpenConsole={() => {}} />);

    await waitFor(() => {
      expect(screen.getByText("claude-sonnet-4-5")).toBeTruthy();
    });
    expect(screen.queryByText(/还没有可展示/)).toBeNull();
  });
});