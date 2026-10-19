import type { AppId } from "@/lib/api/types";

/**
 * Grok Build providers created before the provider-icon rules were aligned
 * received the Grok app icon automatically. The icon picker records the
 * selected icon's default color (`currentColor` for Grok), so an empty color
 * identifies the old automatic value without hiding an explicit user choice.
 */
export function resolveProviderIcon(
  appId: AppId,
  icon?: string,
  iconColor?: string,
): string | undefined {
  const normalizedIcon = icon?.trim();
  if (!normalizedIcon) return undefined;

  if (
    appId === "grokbuild" &&
    normalizedIcon === "grok" &&
    !iconColor?.trim()
  ) {
    return undefined;
  }

  return normalizedIcon;
}

/**
 * Pick a brand mark for a provider row that predates the icon field.
 *
 * The default row written from a live config has no `icon`, so the card used
 * to show a single letter. Matching the relay host is enough to put the right
 * mark back without a data migration.
 */
export function inferProviderIcon(
  name?: string,
  websiteUrl?: string,
  endpoint?: string,
): string | undefined {
  const haystack =
    `${name ?? ""} ${websiteUrl ?? ""} ${endpoint ?? ""}`.toLowerCase();
  if (haystack.includes("aimyria")) return "aimyria";
  return undefined;
}
