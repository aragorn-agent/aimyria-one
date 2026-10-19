/**
 * The clients Aimyria supports, in the order the product presents them.
 *
 * This file is deliberately dependency-free: it is shared by the React UI and
 * mirrored by `src-tauri/src/aimyria_apps.rs`. The two copies are small because
 * that is cheaper than a build step that generates Rust from TypeScript, and
 * `tests/lib/aimyriaApps.test.ts` pins the identifiers the backend accepts.
 *
 * `workbuddy` and `dsh` are Aimyria-specific targets. The Rust enum already has
 * the other eight (`AppType`); those two are implemented by the Aimyria writer
 * rather than pretending to be CC Switch clients.
 */

export type AimyriaAppId =
  | "claude"
  | "claude-desktop"
  | "codex"
  | "workbuddy"
  | "dsh"
  | "hermes"
  | "opencode"
  | "openclaw"
  | "gemini";

export type AimyriaAppGroup = "coding" | "desktop" | "agent";

export interface AimyriaApp {
  id: AimyriaAppId;
  /** Product name; brand names are not translated. */
  name: string;
  /** Extra line under the name in the chooser. */
  subtitle: string;
  /** Terminal / desktop / agent family, used for grouping and badges. */
  group: AimyriaAppGroup;
  /** One-line protocol description, shown in the confirmation dialog. */
  protocol: string;
  /** Whether the app is a first-class target in the chooser. */
  primary: boolean;
}

export const AIMYRIA_APPS: readonly AimyriaApp[] = [
  {
    id: "claude",
    name: "Claude Code",
    subtitle: "终端版",
    group: "coding",
    protocol: "Anthropic Messages",
    primary: true,
  },
  {
    id: "claude-desktop",
    name: "Claude Desktop",
    subtitle: "桌面版",
    group: "desktop",
    protocol: "Anthropic Messages",
    primary: true,
  },
  {
    id: "codex",
    name: "Codex",
    subtitle: "CLI / 桌面端",
    group: "coding",
    protocol: "OpenAI Responses",
    primary: true,
  },
  {
    id: "workbuddy",
    name: "WorkBuddy",
    subtitle: "腾讯 AI 工作台",
    group: "desktop",
    protocol: "OpenAI Compatible",
    primary: true,
  },
  {
    id: "dsh",
    name: "DeepSeek Harness",
    subtitle: "dsh",
    group: "coding",
    protocol: "OpenAI Chat Completions",
    primary: true,
  },
  {
    id: "hermes",
    name: "Hermes",
    subtitle: "Agent + Memory",
    group: "agent",
    protocol: "OpenAI Chat Completions",
    primary: true,
  },
  {
    id: "opencode",
    name: "OpenCode",
    subtitle: "终端编码代理",
    group: "coding",
    protocol: "OpenAI Compatible",
    primary: false,
  },
  {
    id: "openclaw",
    name: "OpenClaw",
    subtitle: "终端代理",
    group: "agent",
    protocol: "OpenAI Chat Completions",
    primary: false,
  },
  {
    id: "gemini",
    name: "Gemini CLI",
    subtitle: "Gemini 原生",
    group: "coding",
    protocol: "Gemini",
    primary: false,
  },
] as const;

export const PRIMARY_AIMYRIA_APPS = AIMYRIA_APPS.filter((app) => app.primary);

export function findAimyriaApp(id: string): AimyriaApp | undefined {
  return AIMYRIA_APPS.find((app) => app.id === id);
}

/**
 * The endpoint each protocol expects.
 *
 * The relay's public base URL is `https://api.aimyria.com` and its OpenAI-
 * compatible surface is `/v1`. Claude's client appends `/v1/messages` itself,
 * so it must receive the origin without `/v1`; this was the documented
 * `/v1/v1/messages` 404 in the website quick-config comments.
 */
export function endpointFor(app: AimyriaAppId, relayBase: string): string {
  const base = relayBase.trim().replace(/\/+$/, "");
  switch (app) {
    case "claude":
    case "claude-desktop":
      return base.replace(/\/v1$/, "");
    case "gemini":
      return base;
    default:
      return /\/v1$/.test(base) ? base : `${base}/v1`;
  }
}

/** A model id with enough structure to choose a primary model per family. */
export interface ModelCandidate {
  id: string;
  /** Context length, when the relay knows it. */
  context?: number;
}

const DATE_SUFFIX = /-(?:\d{8}|\d{4}-\d{2}-\d{2})$/;
const UTILITY = /(?:^|[-_])(?:image|embedding|rerank|tts|whisper|audio|video|compact|review)(?:$|[-_])/i;

function versionOf(id: string): [number, number, number] {
  const match = id.match(/(\d+)(?:[.](\d+))?(?:[.](\d+))?/);
  if (!match) return [0, 0, 0];
  return [
    Number(match[1] ?? 0),
    Number(match[2] ?? 0),
    Number(match[3] ?? 0),
  ];
}

function compareVersionDesc(a: string, b: string): number {
  const av = versionOf(a);
  const bv = versionOf(b);
  for (let i = 0; i < 3; i += 1) {
    if (av[i] !== bv[i]) return bv[i] - av[i];
  }
  return a.localeCompare(b);
}

function usableChatModels(models: readonly string[]): string[] {
  return models.filter(
    (id) =>
      Boolean(id) &&
      !DATE_SUFFIX.test(id) &&
      !UTILITY.test(id) &&
      !/^(?:text-embedding|dall-e|whisper|tts|omni-moderation)/i.test(id),
  );
}

function pickByPrefix(models: readonly string[], prefix: RegExp): string | undefined {
  return usableChatModels(models)
    .filter((id) => prefix.test(id))
    .sort(compareVersionDesc)[0];
}

/**
 * Pick the newest model for a client family.
 *
 * The relay returns a group whitelist, which may contain several families at
 * once (a "国模" group can hold DeepSeek, GLM and Kimi). We choose within the
 * family the client actually speaks, never across families: `glm-5.3` and
 * `deepseek-v4.1` cannot be ordered by their version numbers.
 */
export function pickModelForApp(
  app: AimyriaAppId,
  models: readonly string[],
): string | undefined {
  const usable = usableChatModels(models);
  if (usable.length === 0) return undefined;

  switch (app) {
    case "claude":
    case "claude-desktop":
      return (
        pickByPrefix(usable, /^claude-sonnet/i) ??
        pickByPrefix(usable, /^claude-opus/i) ??
        pickByPrefix(usable, /^claude-/i) ??
        pickByPrefix(usable, /^anthropic\/claude-/i) ??
        usable.sort(compareVersionDesc)[0]
      );
    case "gemini":
      return (
        pickByPrefix(usable, /^gemini-/i) ??
        pickByPrefix(usable, /^models\/gemini-/i) ??
        pickByPrefix(usable, /^gemini/i) ??
        usable.sort(compareVersionDesc)[0]
      );
    case "codex":
      return (
        pickByPrefix(usable, /^(?:openai\/)?gpt-/i) ??
        pickByPrefix(usable, /^(?:o\d|codex)/i) ??
        usable.sort(compareVersionDesc)[0]
      );
    case "workbuddy":
    case "dsh":
    case "hermes":
    case "opencode":
    case "openclaw":
      return (
        pickByPrefix(usable, /^deepseek-/i) ??
        pickByPrefix(usable, /^gpt-/i) ??
        pickByPrefix(usable, /^glm-/i) ??
        pickByPrefix(usable, /^kimi-/i) ??
        pickByPrefix(usable, /^qwen/i) ??
        usable.sort(compareVersionDesc)[0]
      );
  }
}

/**
 * The Claude Code slot mapping, newest-per-role.
 *
 * Claude Code sends different model aliases for the main loop, planning and
 * background calls. Filling all four slots from the group whitelist is what
 * prevents a 404 on the first sub-agent request while the main model works.
 */
export interface ClaudeModelSlots {
  model?: string;
  sonnetModel?: string;
  opusModel?: string;
  haikuModel?: string;
}

export function pickClaudeSlots(models: readonly string[]): ClaudeModelSlots {
  const usable = usableChatModels(models);
  const sonnet = pickByPrefix(usable, /^claude-sonnet/i);
  const opus = pickByPrefix(usable, /^claude-opus/i);
  const haiku = pickByPrefix(usable, /^claude-haiku/i);
  const fallback = sonnet ?? opus ?? haiku ?? usable.sort(compareVersionDesc)[0];

  return {
    model: fallback,
    sonnetModel: sonnet ?? fallback,
    opusModel: opus ?? fallback,
    haikuModel: haiku ?? fallback,
  };
}

/**
 * A compact list for dialogs: newest of each family, capped.
 *
 * The whole list is written into additive clients (OpenCode, WorkBuddy); this
 * shorter list is only for summarising what the user will get.
 */
export function summarizeModels(
  models: readonly string[],
  limit = 8,
): string[] {
  const usable = usableChatModels(models).sort(compareVersionDesc);
  return usable.slice(0, limit);
}
