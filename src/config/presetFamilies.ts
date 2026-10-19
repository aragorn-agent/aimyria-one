// Empty PRESET_FAMILIES.
//
// # What was here
//
// A table of thirty-odd preset families — AWS Bedrock, Baidu Qianfan, Kimi, MiniMax,
// SiliconFlow, Tencent Cloud, Volcengine, Xiaomi MiMo, Zhipu GLM and the rest — each
// with a display name, an i18n key and a plan order for the coding-plan toggle.
//
// It existed to label groups of the presets that were removed. With no presets it
// labels nothing, and every one of those names is a competitor that now ships in the
// bundle for no reason — the last of them, `"xiaomi-mimo": { name: "Xiaomi MiMo" }`,
// was what kept the new built-renderer brand rule red after the 1.35 MB logo
// directory had already gone.
//
// # Why the table stays rather than being deleted
//
// `PresetFamilyId` is `keyof typeof PRESET_FAMILIES`, and the provider config type
// has a `family?: PresetFamilyId` field that is still written by upstream's forms.
// Deleting the binding would break the type and everything that touches it. An
// empty table keeps `PresetFamilyId` as a valid, currently-uninhabited string
// union: if a family is ever wanted back it is one entry, and until then nothing
// downstream has to change.

export interface PresetFamilyInfo {
  name: string;
  nameKey?: string;
  /** Plan order for the coding-plan toggle; overrides the default when present. */
  planOrder?: readonly PresetPlanKey[];
}

export const PRESET_FAMILIES = {} as const satisfies Record<string, PresetFamilyInfo>;

export type PresetFamilyId = keyof typeof PRESET_FAMILIES;

/** Regions a preset family can be published for. Displayed as `providerPreset.region.<key>`. */
export const PRESET_REGION_KEYS = ["cn", "intl"] as const;

export type PresetRegionKey = (typeof PRESET_REGION_KEYS)[number];

// The plans a family can offer. These came from the entries that were removed;
// the union is kept so `planOrder` stays a closed set rather than a loose string,
// which is what `presetGroups.ts` indexes its maps on.
export type PresetPlanKey = "payg" | "coding" | "tokenPlan" | "enterpriseLite" | "enterprisePro";

/**
 * The fields every preset carries.
 *
 * Kept even with the presets gone: `presetGroups.ts` groups by `family`, and the
 * provider config type has `family?: PresetFamilyId`, so the shape is still read
 * and written by the forms. An empty `PRESET_FAMILIES` above means the union of ids
 * is currently uninhabited — nothing can produce a value for this field, and
 * nothing downstream has to change for that to be true.
 */
export interface PresetFamilyFields {
  /** Family; presets of one family merge into a group, only one edition is kept. */
  family?: PresetFamilyId;
  /** The plan this entry sells. Only set when a family offers more than one. */
  planKey?: PresetPlanKey;
  /** The region this entry is published for; drives the `region.<key>` label. */
  regionKey?: PresetRegionKey;
}
