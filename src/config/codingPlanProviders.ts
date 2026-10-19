/**
 * Coding Plan 供应商的 base_url 路由表。
 *
 * 与后端 `src-tauri/src/services/coding_plan.rs::detect_provider` 保持一致：
 * 后端靠 `url.contains(...)` 做子串判断（MiniMax 例外，按 host 标签匹配），
 * 前端这里用 RegExp 做同效匹配。新增供应商时改这一处即可（UsageScriptModal
 * 下拉 + useProviderActions 新建自动注入 + 托盘识别全部复用）。
 *
 * # 为什么表是空的
 *
 * 九家中转都在这里过——Kimi For Coding、Zhipu、MiniMax、Zenmux、Volcengine、
 * OpenCode Go、Command Code——各带一条 base_url 正则，用来在用户粘贴端点时认出
 * 是哪家的编程套餐，从而显示配额、自动注入用量脚本。
 *
 * 九条模式没有一条匹配 `api.aimyria.com`；而预设清空之后，这张表也识别不出任何
 * 别的东西。删掉 1.35 MB logo、预设家族表、搜索别名之后，它是 built renderer 里
 * 最后一个竞品品牌。
 *
 * # 三个函数为什么还在
 *
 * 它们跟表无关：`detectCodingPlanProvider` 遍历一张空表必然返回 null，
 * `injectCodingPlanUsageScript` 见到 null 就原样返回——这两条路径都没坏，只是
 * 现在对任何输入都走"不注入"分支。`extractBaseUrlForUsageDetection` 从五个 app 的
 * settingsConfig 里取 base_url，托盘用量识别仍在用它。
 *
 * 第一个版本只留了空表、把这三个函数整段删了，于是六处调用一起编译失败——和今天
 * 删预设测试、重写 presetFamilies 时犯的是同一个错：改一个文件就把它的其余导出
 * 一起抹掉。这次把整份抄回来，只把表清空。
 */
import { createUsageScript } from "@/types";
import { TEMPLATE_TYPES } from "@/config/constants";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";

export interface CodingPlanProviderEntry {
  /** 与后端 QuotaTier 的 `codingPlanProvider` 取值对齐 */
  id:
    | "kimi"
    | "zhipu"
    | "zhipu_team"
    | "minimax"
    | "zenmux"
    | "volcengine"
    | "opencode_go"
    | "command_code";
  /** UsageScriptModal 下拉显示用 */
  label: string;
  /** base_url 匹配规则 */
  pattern: RegExp;
}

export const CODING_PLAN_PROVIDERS: readonly CodingPlanProviderEntry[] = [];

/** 根据 Base URL 自动检测 Coding Plan 供应商；未命中返回 null */
export function detectCodingPlanProvider(
  baseUrl: string | undefined | null,
): CodingPlanProviderEntry["id"] | null {
  if (!baseUrl) return null;
  for (const cp of CODING_PLAN_PROVIDERS) {
    if (cp.pattern.test(baseUrl)) return cp.id;
  }
  return null;
}

/** 按 app 从 settingsConfig 里取出 base_url，供自动注入检测用。 */
export function extractBaseUrlForUsageDetection(
  appId: string,
  settingsConfig: Record<string, any> | undefined,
): string | null {
  if (!settingsConfig) return null;
  let raw: unknown;
  switch (appId) {
    case "claude":
    case "claude-desktop":
      raw = settingsConfig.env?.ANTHROPIC_BASE_URL;
      break;
    case "codex":
      raw = extractCodexBaseUrl(
        typeof settingsConfig.config === "string"
          ? settingsConfig.config
          : null,
      );
      break;
    case "opencode":
      raw = settingsConfig.options?.baseURL;
      break;
    case "pi":
      raw = settingsConfig.baseUrl;
      break;
    default:
      return null;
  }
  return typeof raw === "string" ? raw : null;
}

/**
 * 新建供应商时，若 base_url 命中 Coding Plan 路由表，自动把
 * `meta.usage_script` 标记为 token_plan 并启用。
 *
 * 现在这张表是空的，所以这个函数对任何输入都原样返回——保留调用点不动，是因为
 * 调用点本身仍然正确：一旦表里重新有条目，注入逻辑立刻恢复，不需要再改三处。
 */
export function injectCodingPlanUsageScript<
  T extends {
    settingsConfig?: Record<string, any>;
    meta?: Record<string, any>;
  },
>(appId: string, provider: T): T {
  if (provider.meta?.usage_script) return provider;

  const baseUrl = extractBaseUrlForUsageDetection(
    appId,
    provider.settingsConfig,
  );
  const codingPlanProvider = detectCodingPlanProvider(baseUrl);
  if (!codingPlanProvider) return provider;
  const isCommandCodeForCodex =
    appId === "codex" && codingPlanProvider === "command_code";
  if (
    appId !== "claude" &&
    codingPlanProvider !== "opencode_go" &&
    !isCommandCodeForCodex
  ) {
    return provider;
  }

  return {
    ...provider,
    meta: {
      ...(provider.meta ?? {}),
      usage_script: createUsageScript({
        enabled: true,
        templateType: TEMPLATE_TYPES.TOKEN_PLAN,
        codingPlanProvider,
      }),
    },
  };
}
