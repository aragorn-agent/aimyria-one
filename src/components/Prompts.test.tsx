/**
 * Tests for the Prompts screen.
 *
 * Two things are easy to get wrong here and both mislead the user silently:
 * showing another client's prompts when the scope changed, and showing an
 * enabled prompt that the backend never wrote to the client's file.
 */

import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { Prompts } from "./Prompts";
import type { Prompt } from "../lib/prompts";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
}));

const invoke = vi.mocked((await import("@tauri-apps/api/core")).invoke);

/**
 * `invoke` is generic in its return type, so a `mockImplementation` that answers
 * different commands with different shapes makes TypeScript infer a union the
 * mock's own signature rejects. Widening to `unknown` once here keeps each
 * test's implementation readable.
 */
type Handler = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
const onInvoke = (fn: Handler) => invoke.mockImplementation(fn as never);

const prompt = (over: Partial<Prompt> = {}): Prompt => ({
  id: "concise",
  name: "简洁回答",
  content: "回答控制在三句话以内。",
  enabled: false,
  ...over,
});

beforeEach(() => {
  invoke.mockReset();
});

describe("Prompts", () => {
  it("shows only the selected client's library", async () => {
    onInvoke(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "get_prompts") {
        // The screen asks for one client at a time; answering with a
        // client-specific library is what proves it respects that scope.
        return args?.app === "claude"
          ? { concise: prompt() }
          : { terse: prompt({ id: "terse", name: "极简模式" }) };
      }
      if (cmd === "get_prompt_file_location") {
        return { path: "C:/x/CLAUDE.md", displayPath: "~/CLAUDE.md" };
      }
      return null;
    });

    render(<Prompts />);

    await waitFor(() => expect(screen.getByText("简洁回答")).toBeTruthy());
    // Not shown: that is Codex's library, not Claude's.
    expect(screen.queryByText("极简模式")).toBeNull();
    expect(screen.getByText(/~\/CLAUDE\.md/)).toBeTruthy();
  });

  it("marks the enabled prompt, and shows enable only on the others", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_prompts") {
        return {
          concise: prompt({ enabled: true }),
          verbose: prompt({ id: "verbose", name: "详细说明" }),
        };
      }
      if (cmd === "get_prompt_file_location") {
        return { path: "C:/x/CLAUDE.md", displayPath: "~/CLAUDE.md" };
      }
      return null;
    });

    render(<Prompts />);

    await waitFor(() => expect(screen.getByText("使用中")).toBeTruthy());

    // Enabling is exclusive — the backend demotes the rest — so only the
    // disabled row offers the action. Two "启用" buttons would be a lie.
    const enables = screen.getAllByRole("button", { name: "启用" });
    expect(enables).toHaveLength(1);
  });

  it("an empty library says so and offers the way in", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_prompts") return {};
      if (cmd === "get_prompt_file_location") {
        return { path: "C:/x/CLAUDE.md", displayPath: "~/CLAUDE.md" };
      }
      return null;
    });

    render(<Prompts />);

    await waitFor(() => expect(screen.getByRole("button", { name: "从文件导入" })).toBeTruthy());
    // The save button stays disabled until there is something to save.
    expect(screen.getByRole("button", { name: "保存" })).toHaveProperty("disabled", true);
  });

  it("a failed read is reported, not shown as an empty library", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_prompt_file_location") {
        return { path: "C:/x/CLAUDE.md", displayPath: "~/CLAUDE.md" };
      }
      throw new Error("db locked");
    });

    render(<Prompts />);

    await waitFor(() => expect(screen.getByText(/读取失败/)).toBeTruthy());
    expect(screen.queryByText(/还没有提示词/)).toBeNull();
  });

  it("switching client re-reads, rather than keeping the previous library", async () => {
    const calls: Array<{ cmd: string; app?: unknown }> = [];
    onInvoke(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "get_prompts") {
        calls.push({ cmd, app: args?.app });
        return args?.app === "claude" ? { concise: prompt() } : {};
      }
      if (cmd === "get_prompt_file_location") {
        return { path: "C:/x", displayPath: "~/x" };
      }
      return null;
    });

    render(<Prompts />);
    await waitFor(() => expect(screen.getByText("简洁回答")).toBeTruthy());

    // Switching client re-reads, so the swap settles over several ticks.
    await act(async () => {
      screen.getByRole("tab", { name: "Codex" }).click();
    });

    // The old library must go. Leaving Claude's prompt visible under a Codex
    // heading is the failure this guards.
    await waitFor(() => expect(screen.getByText(/还没有提示词/)).toBeTruthy());
    expect(calls.map((c) => c.app)).toEqual(["claude", "codex"]);
  });

  it("enabling asks the backend, then re-reads what it stored", async () => {
    const calls: string[] = [];
    invoke.mockImplementation(async (cmd: string) => {
      calls.push(cmd);
      if (cmd === "get_prompts") return { concise: prompt() };
      if (cmd === "get_prompt_file_location") {
        return { path: "C:/x", displayPath: "~/x" };
      }
      return null;
    });

    render(<Prompts />);
    await waitFor(() => expect(screen.getByText("简洁回答")).toBeTruthy());

    await act(async () => {
      screen.getByRole("button", { name: "启用" }).click();
    });

    await waitFor(() => expect(calls).toContain("enable_prompt"));
    const idx = calls.indexOf("enable_prompt");
    expect(calls.slice(idx + 1)).toContain("get_prompts");
  });
});
