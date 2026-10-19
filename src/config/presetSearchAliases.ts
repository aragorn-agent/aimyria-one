/**
 * 预设搜索的别名（中文名、公司名、旧名）：按预设的 `name` 查。只用于添加供应商时的搜索，
 * 不参与显示。新增预设时，名字里搜不到的常用叫法加在这里。
 *
 * # 为什么是空的
 *
 * 33 条别名，每条对应一个已经不存在的预设：AWS Bedrock、Baidu Qianfan、DeepSeek、
 * GitHub Copilot、Xiaomi MiMo、Zhipu GLM……连中文叫法一起（"Xiaomi MiMo" → "小米"）。
 *
 * 一张把竞品中文名映射到常用叫法的表，在产品里没有任何东西可搜。最后活着的证据是
 * 它让新加的「产物里不得出现竞品品牌」规则一直红着：删掉 1.35 MB logo 和预设家族表
 * 之后，`"Xiaomi MiMo":"小米"` 是 bundle 里剩下的最后一个。
 *
 * 常量本身保留——`presetGroups.ts` 每次都查它，删掉导出要改调用方；空表返回空，
 * 语义就是「没有任何预设需要别名」，这正是事实。
 */
export const PRESET_SEARCH_ALIASES: Record<string, string> = {};
