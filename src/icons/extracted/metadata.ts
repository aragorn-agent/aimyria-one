// Icon metadata, reduced to the four clients this build configures.
//
// The previous version carried 66 relay vendors — displayName, category, brand
// colour and search aliases for every competitor on the market. `searchIcons`
// returned them, `IconPicker` rendered them, and they shipped in the bundle.
//
// # Why the vendor colours are gone too
//
// `defaultColor: "#2874FF"` on PPIO and "#005AFF" on StepFun were a lookup table
// of other companies' brand palettes. Even if no logo had survived, those hex
// values would still be our binary describing their identity, and the palette
// brand gate (which bans borrowed blues) would have had to start making
// exceptions. Four clients need four colours, and they use ours.

export interface IconMetadata {
  name: string;
  displayName: string;
  category: string;
  keywords: string[];
  defaultColor: string;
  /** One of the four clients is drawn as a tile; the rest are plain marks. */
  isTile: boolean;
}

// Only clients. Each entry names what it is, so a future reader does not have to
// guess whether "openai" means Codex the client or OpenAI the relay — it is here
// for Codex, which sends an OpenAI-shaped protocol.
export const iconMetadata: Record<string, IconMetadata> = {
  claude: {
    name: "claude",
    displayName: "Claude",
    category: "ai-client",
    keywords: ["claude", "anthropic"],
    defaultColor: "currentColor",
    isTile: false,
  },
  openai: {
    name: "openai",
    displayName: "Codex",
    category: "ai-client",
    keywords: ["codex", "openai", "gpt"],
    defaultColor: "currentColor",
    isTile: false,
  },
  gemini: {
    name: "gemini",
    displayName: "Gemini",
    category: "ai-client",
    keywords: ["gemini", "google"],
    defaultColor: "currentColor",
    isTile: false,
  },
  openclaw: {
    name: "openclaw",
    displayName: "OpenClaw",
    category: "ai-client",
    keywords: ["openclaw", "claw"],
    defaultColor: "currentColor",
    isTile: false,
  },
  grok: {
    name: "grok",
    displayName: "Grok Build",
    category: "ai-client",
    keywords: ["grok", "xai", "x"],
    defaultColor: "currentColor",
    isTile: false,
  },
  minimax: {
    name: "minimax",
    displayName: "MiniMax Code",
    category: "ai-client",
    keywords: ["minimax", "mcode"],
    defaultColor: "currentColor",
    isTile: false,
  },
  opencode: {
    name: "opencode",
    displayName: "OpenCode",
    category: "ai-client",
    keywords: ["opencode"],
    defaultColor: "currentColor",
    isTile: false,
  },
  aimyria: {
    name: "aimyria",
    displayName: "AIMYRIA",
    category: "relay",
    keywords: ["aimyria"],
    defaultColor: "currentColor",
    isTile: false,
  },
  workbuddy: {
    name: "workbuddy",
    displayName: "WorkBuddy",
    category: "ai-client",
    keywords: ["workbuddy", "tencent"],
    defaultColor: "currentColor",
    isTile: false,
  },
};

export function getIconMetadata(name: string): IconMetadata | undefined {
  return iconMetadata[name];
}

export function isTileIcon(name?: string): boolean {
  return name ? (iconMetadata[name]?.isTile ?? false) : false;
}

export function searchIcons(query: string): string[] {
  const q = query.trim().toLowerCase();
  if (!q) return Object.keys(iconMetadata);
  return Object.values(iconMetadata)
    .filter(
      (m) =>
        m.displayName.toLowerCase().includes(q) ||
        m.keywords.some((k) => k.includes(q)),
    )
    .map((m) => m.name);
}
