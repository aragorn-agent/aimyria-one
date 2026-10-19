import { describe, expect, it } from "vitest";

import {
  AIMYRIA_APPS,
  PRIMARY_AIMYRIA_APPS,
  endpointFor,
  pickClaudeSlots,
  pickModelForApp,
  summarizeModels,
} from "@/shared/aimyriaApps";

describe("Aimyria app catalogue", () => {
  it("lists the six first-class targets with Claude split by surface", () => {
    expect(PRIMARY_AIMYRIA_APPS.map((app) => app.id)).toEqual([
      "claude",
      "claude-desktop",
      "codex",
      "workbuddy",
      "dsh",
      "hermes",
    ]);
    expect(
      PRIMARY_AIMYRIA_APPS.find((app) => app.id === "claude")?.subtitle,
    ).toContain("终端");
    expect(
      PRIMARY_AIMYRIA_APPS.find((app) => app.id === "claude-desktop")?.subtitle,
    ).toContain("桌面");
  });

  it("keeps every id unique", () => {
    const ids = AIMYRIA_APPS.map((app) => app.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});

describe("endpointFor", () => {
  it("never gives Claude a /v1 suffix", () => {
    expect(endpointFor("claude", "https://api.aimyria.com/v1/")).toBe(
      "https://api.aimyria.com",
    );
    expect(endpointFor("claude-desktop", "https://api.aimyria.com")).toBe(
      "https://api.aimyria.com",
    );
  });

  it("adds /v1 exactly once for OpenAI-compatible clients", () => {
    for (const app of [
      "codex",
      "workbuddy",
      "dsh",
      "hermes",
      "opencode",
      "openclaw",
    ] as const) {
      expect(endpointFor(app, "https://api.aimyria.com")).toBe(
        "https://api.aimyria.com/v1",
      );
      expect(endpointFor(app, "https://api.aimyria.com/v1")).toBe(
        "https://api.aimyria.com/v1",
      );
    }
  });
});

describe("model selection", () => {
  const group = [
    "claude-sonnet-4-6",
    "claude-opus-5",
    "claude-haiku-4-5",
    "claude-sonnet-5-5",
    "gpt-6.1-sol",
    "gpt-5.6",
    "deepseek-v4.1-flash",
    "deepseek-v4-pro",
    "glm-5.3",
    "gpt-image-1",
  ];

  it("picks the newest Claude Sonnet for Claude Code", () => {
    expect(pickModelForApp("claude", group)).toBe("claude-sonnet-5-5");
  });

  it("fills every Claude slot so sub-agents do not 404", () => {
    expect(pickClaudeSlots(group)).toEqual({
      model: "claude-sonnet-5-5",
      sonnetModel: "claude-sonnet-5-5",
      opusModel: "claude-opus-5",
      haikuModel: "claude-haiku-4-5",
    });
  });

  it("prefers GPT for Codex", () => {
    expect(pickModelForApp("codex", group)).toBe("gpt-6.1-sol");
  });

  it("prefers DeepSeek for a mixed Chinese-model group", () => {
    expect(pickModelForApp("workbuddy", group)).toBe("deepseek-v4.1-flash");
    expect(pickModelForApp("dsh", group)).toBe("deepseek-v4.1-flash");
  });

  it("excludes image and dated utility models", () => {
    expect(
      summarizeModels([
        "gpt-image-1",
        "claude-sonnet-5-20260101",
        "claude-sonnet-5",
      ]),
    ).toEqual(["claude-sonnet-5"]);
  });

  it("returns undefined for an empty whitelist", () => {
    expect(pickModelForApp("codex", [])).toBeUndefined();
  });
});
