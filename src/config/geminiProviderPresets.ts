import type { ProviderCategory } from "@/types";
import type { PresetFamilyFields } from "./presetFamilies";

/**
 * Gemini 预设供应商的视觉主题配置
 */
export interface GeminiPresetTheme {
  /** 图标类型：'gemini' | 'generic' */
  icon?: "gemini" | "generic";
  /** 背景色（选中状态），支持 hex 颜色 */
  backgroundColor?: string;
  /** 文字色（选中状态），支持 hex 颜色 */
  textColor?: string;
}

export interface GeminiProviderPreset extends PresetFamilyFields {
  name: string;
  nameKey?: string; // i18n key for localized display name
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: object;
  baseURL?: string;
  model?: string;
  description?: string;
  category?: ProviderCategory;
  isPartner?: boolean;
  primePartner?: boolean; // 旧版的置顶合作伙伴标记；v7 起界面不再读取，新预设不写
  partnerPromotionKey?: string;
  endpointCandidates?: string[];
  theme?: GeminiPresetTheme;
  // 图标配置
  icon?: string; // 图标名称
  iconColor?: string; // 图标颜色
}

// No third-party relay presets: a preset is our recommendation, and this build
// ships exactly one relay — Aimyria.
//
// The official entry is the user's own Google account; empty `env` + `config`
// lets the Gemini CLI use its own Google OAuth login rather than a key we ship.
export const geminiOfficialPreset: GeminiProviderPreset = {
  name: "Google Official",
  websiteUrl: "https://ai.google.dev/",
  apiKeyUrl: "https://aistudio.google.com/apikey",
  settingsConfig: { env: {}, config: {} },
  description: "Google 官方 Gemini API (OAuth)",
  category: "official",
  icon: "gemini",
  iconColor: "#4285F4",
};

export const geminiProviderPresets: GeminiProviderPreset[] = [
  geminiOfficialPreset,
];

export function getGeminiPresetByName(
  name: string,
): GeminiProviderPreset | undefined {
  return geminiProviderPresets.find((preset) => preset.name === name);
}

export function getGeminiPresetByUrl(
  url: string,
): GeminiProviderPreset | undefined {
  if (!url) return undefined;
  return geminiProviderPresets.find(
    (preset) =>
      preset.baseURL &&
      url.toLowerCase().includes(preset.baseURL.toLowerCase()),
  );
}
