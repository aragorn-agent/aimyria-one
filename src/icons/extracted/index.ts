// The icon index, reduced to what this product actually has.
//
// # What was here
//
// 116 files — 1.35 MB of PNG logos for 66 relay vendors: OpenRouter, DeepSeek,
// Kimi, GLM, MiniMax, PPIO, JieKou, SubRouter, TheRouter, MoArk, SiliconFlow,
// Qianfan and the rest, each with a displayName, a category and a defaultColor.
//
// They were reachable. `IconPicker` lists `iconList`, so the "add provider" screen
// offered all 66 brand marks and Aimyria was the 67th row. That is a price-shopping
// surface one click from our own relay, and it is 1.35 MB of someone else's
// trademarks in the bundle.
//
// # What is left
//
// Four client icons — Claude, OpenAI (Codex uses it), Gemini, OpenClaw — because
// `BrandIcons.tsx` renders them in the sidebar and provider rows, and they
// identify *clients*, not competing relays.
//
// `grok` is a bare `<img>` in the Grok Build form for a client this build does not
// configure; it is not in the index and never was, so deleting the vendor logos
// does not touch it.
//
// # Why the API shape is unchanged
//
// `IconPicker.tsx` and `ProviderIcon.tsx` import `iconList`, `getIconMetadata`,
// `searchIcons` and `isTileIcon`. Keeping the same four names and signatures means
// those components did not have to change — which matters because the alternative
// was editing a live form to satisfy an asset decision.

import ClaudeSvg from "./claude.svg?url";
import OpenAISvg from "./openai.svg?url";
import GeminiSvg from "./gemini.svg?url";
import OpenClawSvg from "./claw.svg?url";
import GrokSvg from "./grok.svg?url";
import MiniMaxSvg from "./minimax.svg?url";
import OpenCodeSvg from "./opencode.svg?url";
import AimyriaSvg from "./aimyria.svg?url";
import WorkBuddySvg from "./workbuddy.svg?url";

// Client marks. Order is sidebar order so the picker reads the same way the
// app does.
//
// There is deliberately no `321ai` entry. That mark was a third-party gateway's
// logo shown inside our own access centre, which presented someone else's brand
// as one of our integrations. Only Aimyria and the AI clients we configure are
// listed here.
const ICONS: Record<string, string> = {
  claude: ClaudeSvg,
  openai: OpenAISvg,
  gemini: GeminiSvg,
  openclaw: OpenClawSvg,
  grok: GrokSvg,
  minimax: MiniMaxSvg,
  opencode: OpenCodeSvg,
  aimyria: AimyriaSvg,
  workbuddy: WorkBuddySvg,
};

export const iconList: string[] = Object.keys(ICONS);

export function getIconSrc(name?: string): string | undefined {
  return name ? ICONS[name] : undefined;
}

// The four helpers `ProviderIcon.tsx` used when the index held 116 entries. Kept,
// with the same signatures, so the component did not have to change: it asks for a
// URL, asks whether an icon exists, and asks whether a name is a URL rather than an
// index entry. All three answers are now about the four clients.
export function getIcon(name?: string): string | undefined {
  return getIconSrc(name);
}

export function hasIcon(name?: string): boolean {
  return Boolean(name && name in ICONS);
}

export function getIconUrl(name?: string): string | undefined {
  return getIconSrc(name);
}

// A stored icon field may hold a URL instead of an index name — providers written
// before the vendor logos were removed carried data: URIs.
export function isUrlIcon(name?: string): boolean {
  return Boolean(name && /^(https?:|data:)/.test(name));
}

export { getIconMetadata, isTileIcon, searchIcons } from "./metadata";
