/**
 * Tests for the Skills screen.
 *
 * The screen's job is to say which skills are written into which clients. Both
 * failure modes worth guarding are quiet ones: rendering an empty table when the
 * read failed, and rendering a checked box that the backend never wrote.
 *
 * `skillsApi` is mocked so these run in jsdom without a Tauri runtime.
 */

import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { Skills } from "./Skills";
import type { InstalledSkill } from "../lib/skills";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
}));

const invoke = vi.mocked((await import("@tauri-apps/api/core")).invoke);

const skill = (over: Partial<InstalledSkill> = {}): InstalledSkill => ({
  id: "pdf",
  name: "pdf-tools",
  // Deliberately not the same string as the name: a skill's directory is its
  // real identity, and a fixture where the two match would hide a screen that
  // renders the name and silently drops the directory.
  directory: "skills/pdf-tools",
  description: "读 PDF、填表、合并",
  apps: {
    claude: true,
    "claude-desktop": false,
    codex: false,
    gemini: false,
    grokbuild: false,
    opencode: false,
    openclaw: false,
    hermes: false,
    pi: false,
    mcode: false,
  },
  installedAt: 0,
  updatedAt: 0,
  ...over,
});

beforeEach(() => {
  invoke.mockReset();
});

describe("Skills", () => {
  it("renders one check per supported client, from the stored state", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_installed_skills") return [skill()];
      return null;
    });

    render(<Skills />);

    await waitFor(() => expect(screen.getByText("pdf-tools")).toBeTruthy());

    // Five boxes, not ten: the other five clients are not ones this build writes.
    expect(screen.getAllByRole("checkbox")).toHaveLength(5);

    const checked = screen.getAllByRole("checkbox", { checked: true });
    expect(checked).toHaveLength(1);
    expect(checked[0].getAttribute("aria-label")).toContain("Claude Code");
  });

  it("shows the directory, which is what actually lands on disk", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_installed_skills") return [skill()];
      return null;
    });

    render(<Skills />);

    // Not the name — the name is a label. What gets copied into each client is
    // the directory, so that is what has to be on screen.
    await waitFor(() => expect(screen.getByText("skills/pdf-tools")).toBeTruthy());
    expect(screen.getByText(/读 PDF、填表、合并/)).toBeTruthy();
  });

  it("an empty library is said to be empty, with the way out", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_installed_skills") return [];
      return null;
    });

    render(<Skills />);

    // Not a blank table: the empty state has to say what to do next, which is
    // scan — the one action that finds skills the user already has.
    await waitFor(() => expect(screen.getByRole("button", { name: "扫描" })).toBeTruthy());
    expect(screen.queryAllByRole("checkbox")).toHaveLength(0);
  });

  it("a failed read is reported, not shown as an empty library", async () => {
    invoke.mockImplementation(async () => {
      throw new Error("db locked");
    });

    render(<Skills />);

    await waitFor(() => expect(screen.getByText(/读取失败/)).toBeTruthy());
    expect(screen.queryByText(/还没有安装任何 Skill/)).toBeNull();
  });

  it("toggling asks the backend to write, then re-reads what was stored", async () => {
    const calls: string[] = [];
    invoke.mockImplementation(async (cmd: string) => {
      calls.push(cmd);
      if (cmd === "get_installed_skills") return [skill()];
      return null;
    });

    render(<Skills />);
    await waitFor(() => expect(screen.getByText("pdf-tools")).toBeTruthy());

    // A click triggers a write and then a re-read, so the state settles over
    // several ticks. `act` says "let those finish" rather than asserting against
    // a half-updated tree.
    await act(async () => {
      screen.getAllByRole("checkbox")[1].click(); // Codex
    });

    await waitFor(() => expect(calls).toContain("toggle_skill_app"));
    // A write must be followed by a read. The screen reports what is on disk;
    // "optimistic" here would mean guessing what the backend did.
    const idx = calls.indexOf("toggle_skill_app");
    expect(calls.slice(idx + 1)).toContain("get_installed_skills");
  });

  it("a partial resync names the clients that failed", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_installed_skills") return [];
      if (cmd === "resync_skills_to_apps") {
        return [
          { app: "claude", ok: true, failedSkills: [] },
          { app: "codex", ok: false, error: "read-only dir", failedSkills: [] },
        ];
      }
      return null;
    });

    render(<Skills />);
    await waitFor(() => expect(screen.getByRole("button", { name: "重写配置" })).toBeTruthy());

    await act(async () => {
      screen.getByRole("button", { name: "重写配置" }).click();
    });

    // "Done" would be a lie here: one of the two clients did not get written.
    await waitFor(() => expect(screen.getByText(/部分失败/)).toBeTruthy());
    expect(screen.getByText(/Codex/)).toBeTruthy();
  });
});
