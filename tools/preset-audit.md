# 复制进来的 provider 预设

- 预设文件：**16** 个
- 总行数：**1,637**
- `id:` 条目（粗略）：**27**
- 出现的第三方中转名：**21** 种

## 按文件

| 文件 | 行数 | 条目 | 含第三方 |
| --- | ---: | ---: | ---: |
| `opencodeProviderPresets.ts` | 282 | 22 | 4 |
| `presetModelMetadata.ts` | 151 | 0 | 0 |
| `mcodeProviderPresets.ts` | 148 | 0 | 0 |
| `hermesProviderPresets.ts` | 133 | 0 | 0 |
| `mcpPresets.ts` | 107 | 5 | 0 |
| `openclawProviderPresets.ts` | 105 | 0 | 0 |
| `piProviderPresets.ts` | 101 | 0 | 2 |
| `codexProviderPresets.ts` | 92 | 0 | 2 |
| `presetFamilies.ts` | 87 | 0 | 14 |
| `universalProviderPresets.ts` | 87 | 0 | 1 |
| `claudeProviderPresets.ts` | 78 | 0 | 0 |
| `claudeDesktopProviderPresets.ts` | 74 | 0 | 0 |
| `grokBuildProviderPresets.ts` | 61 | 0 | 3 |
| `geminiProviderPresets.ts` | 54 | 0 | 0 |
| `presetSearchAliases.ts` | 53 | 0 | 16 |
| `userAgentPresets.ts` | 24 | 0 | 1 |

## 第三方名出现次数

- Kimi ×4
- MiMo ×3
- SiliconFlow ×3
- DeepSeek ×3
- GLM ×3
- MiniMax ×3
- Qwen ×3
- mimo ×2
- 智谱 ×2
- Zhipu ×2
- Qianfan ×2
- 接口 ×2
- 火山 ×2
- Baidu ×2
- Novita ×1
- OpenRouter ×1
- Volcengine ×1
- 硅基流动 ×1
- 豆包 ×1
- 通义 ×1
- Longcat ×1

## 与此相反的：Rust 侧

`providers_seed.rs` 的 `OFFICIAL_SEEDS` 已清空——品牌门禁查的就是它，所以
「No competitor presets are seeded」一直是 PASS。它没看 `src/config/`。
