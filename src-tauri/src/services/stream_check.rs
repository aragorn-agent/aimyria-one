//! 供应商连通性检查服务（reachability）
//!
//! 仅探测供应商 `base_url` 是否可达，**不发送真实大模型请求**：
//! - 收到任意 HTTP 响应（200/4xx/5xx）即判定"可达"（端口通、网关存活）；
//! - 仅 DNS / 连接被拒 / TLS / 超时等网络级错误判定"不可达"；
//! - 延迟 = 收到响应头的耗时（TTFB，真实往返）。
//!
//! ## 设计取舍：可达 ≠ 配置正确
//!
//! 本检查刻意不验证鉴权或模型，因此不会被第三方供应商的鉴权拦截 / 模型校验
//! 误判为"不可用"。代价是它无法告诉你鉴权对不对、模型存不存在。
//!
//! ## 与故障转移的关系（重要不变量）
//!
//! 连通性检查 **绝不** 触碰故障转移熔断器：一个返回 403/401 的供应商在本检查里
//! 算"可达"，但它对真实流量是坏的。熔断器只由 `proxy/forwarder.rs` 转发真实流量
//! 的成败驱动（被动）。两者职责分离——可达性回答"能不能到"，真实流量回答"能不能用"。

use reqwest::header::HeaderValue;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;

/// 健康状态枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Operational,
    Degraded,
    Failed,
}

/// 连通性检查参数（固定默认值，不对用户开放）
#[derive(Debug, Clone)]
pub struct StreamCheckConfig {
    /// 单次探测超时（秒）
    pub timeout_secs: u64,
    /// 超时类失败的最大重试次数
    pub max_retries: u32,
    /// 降级阈值（毫秒）：可达但 TTFB 超过该值判定为"较慢"
    pub degraded_threshold_ms: u64,
}

impl Default for StreamCheckConfig {
    fn default() -> Self {
        // 可达性探测打的是 base_url 的小请求（仅读响应头），不等待模型生成，故超时远小于
        // 旧的真实请求检查（45s → 8s）；降级阈值沿用旧尺度 6000ms——探测 TTFB 一般远低于
        // 此，仅在确实很慢时才标"较慢"，避免把 1 秒多的正常延迟误判为降级。
        Self {
            timeout_secs: 8,
            max_retries: 1,
            degraded_threshold_ms: 6000,
        }
    }
}

/// 连通性检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamCheckResult {
    pub status: HealthStatus,
    pub success: bool,
    pub message: String,
    pub response_time_ms: Option<u64>,
    pub http_status: Option<u16>,
    pub tested_at: i64,
    pub retry_count: u32,
}

/// 连通性检查服务
pub struct StreamCheckService;

impl StreamCheckService {
    /// 执行连通性检查（仅对超时类失败重试）。
    ///
    /// `base_url_override`：用于 Copilot 等需要从 OAuth 管理器动态解析端点的供应商，
    /// 由命令层预先解析后传入；其余供应商传 `None`，由本服务从 `settings_config` 提取。
    pub async fn check_with_retry(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
        base_url_override: Option<String>,
    ) -> Result<StreamCheckResult, AppError> {
        let mut last_result: Option<StreamCheckResult> = None;
        for attempt in 0..=config.max_retries {
            let start = Instant::now();
            let result =
                Self::check_once(app_type, provider, config, base_url_override.clone(), start)
                    .await?;

            if result.success {
                return Ok(StreamCheckResult {
                    retry_count: attempt,
                    ..result
                });
            }

            // 仅超时 / abort 类网络抖动值得重试；连接被拒、DNS 失败等立即返回。
            if Self::should_retry(&result.message) && attempt < config.max_retries {
                last_result = Some(result);
                continue;
            }
            return Ok(StreamCheckResult {
                retry_count: attempt,
                ..result
            });
        }

        Ok(last_result.unwrap_or_else(|| StreamCheckResult {
            status: HealthStatus::Failed,
            success: false,
            message: "Check failed".to_string(),
            response_time_ms: None,
            http_status: None,
            tested_at: chrono::Utc::now().timestamp(),
            retry_count: config.max_retries,
        }))
    }

    /// 单次连通性探测。
    async fn check_once(
        app_type: &AppType,
        provider: &Provider,
        config: &StreamCheckConfig,
        base_url_override: Option<String>,
        start: Instant,
    ) -> Result<StreamCheckResult, AppError> {
        let base_url = match base_url_override {
            Some(b) => b,
            None => Self::resolve_base_url(app_type, provider)?,
        };

        let timeout = std::time::Duration::from_secs(config.timeout_secs);
        let ua = Self::custom_user_agent(provider);

        // A plain GET to the base URL: any HTTP response proves the port is
        // reachable, so there is no need to replay the provider's real call.
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| AppError::Message(format!("failed to build HTTP client: {e}")))?;
        let mut request = client.get(&base_url);
        if let Some(ua) = ua {
            request = request.header(reqwest::header::USER_AGENT, ua);
        }

        let response_time = start.elapsed().as_millis() as u64;
        let status = match request.send().await {
            Ok(resp) => resp.status().as_u16(),
            Err(e) => return Err(Self::map_request_error(e)),
        };
        Ok(Self::build_result(
            Ok(status),
            response_time,
            config.degraded_threshold_ms,
        ))
    }

    /// 解析供应商 `base_url`。
    ///
    /// 连通性探测只需打到 base（origin 或用户配置的 base 路径）即可——任何 HTTP
    /// 响应都证明端口可达，因此无需像旧的真实请求检查那样解析具体 API 路径
    /// （`/v1/messages` vs `/chat/completions` vs `:streamGenerateContent`）。
    ///
    /// 官方供应商（`category == "official"`）base_url 故意留空（走客户端默认/OAuth 端点），
    /// 没有 cc-switch 能可靠探测的目标——这类供应商的连通检测按钮在前端已隐藏
    /// （见 `ProviderCard.tsx`），故此处对其提取失败直接报错即可，不做官方端点回退。
    fn resolve_base_url(app_type: &AppType, provider: &Provider) -> Result<String, AppError> {
        if provider.category.as_deref() == Some("official") {
            return Err(AppError::Message(
                "Official providers do not expose a reachability-check target".to_string(),
            ));
        }

        match app_type {
            AppType::Claude => Self::extract_env_base_url(provider, "claude"),
            AppType::Codex | AppType::GrokBuild => Self::extract_codex_base_url(provider),
            AppType::Gemini => Self::extract_env_base_url(provider, "gemini"),
            AppType::OpenCode => {
                let npm = Self::extract_opencode_npm(provider);
                Self::resolve_opencode_base_url(provider, npm.as_deref())
            }
            AppType::OpenClaw => Self::extract_openclaw_base_url(provider),
            AppType::Hermes => Self::extract_hermes_base_url(provider),
            other => Err(AppError::Message(format!(
                "{} 不支持连通性检测",
                other.as_str()
            ))),
        }
    }

    /// 将探测原始结果包装成 `StreamCheckResult`。
    fn build_result(
        result: Result<u16, AppError>,
        response_time: u64,
        degraded_threshold_ms: u64,
    ) -> StreamCheckResult {
        let tested_at = chrono::Utc::now().timestamp();
        match result {
            Ok(status) => StreamCheckResult {
                status: Self::determine_status(response_time, degraded_threshold_ms),
                success: status < 400,
                message: format!("HTTP {status}"),
                response_time_ms: Some(response_time),
                http_status: Some(status),
                tested_at,
                retry_count: 0,
            },
            Err(error) => StreamCheckResult {
                status: HealthStatus::Failed,
                success: false,
                message: error.to_string(),
                response_time_ms: Some(response_time),
                http_status: None,
                tested_at,
                retry_count: 0,
            },
        }
    }

    fn determine_status(latency_ms: u64, threshold: u64) -> HealthStatus {
        if latency_ms <= threshold {
            HealthStatus::Operational
        } else {
            HealthStatus::Degraded
        }
    }

    fn should_retry(msg: &str) -> bool {
        let lower = msg.to_lowercase();
        lower.contains("timeout") || lower.contains("abort") || lower.contains("timed out")
    }

    fn map_request_error(e: reqwest::Error) -> AppError {
        if e.is_timeout() {
            AppError::Message("Request timeout".to_string())
        } else if e.is_connect() {
            AppError::Message(format!("Connection failed: {e}"))
        } else {
            AppError::Message(e.to_string())
        }
    }

    /// Provider 级自定义 User-Agent（`meta.customUserAgent`），与转发路径共用单一口径：
    /// trim、空串视为未设置、非法值静默忽略（返回 `None`）。
    fn custom_user_agent(provider: &Provider) -> Option<HeaderValue> {
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.custom_user_agent_header().ok().flatten())
    }

    // ===== 各应用 base_url 提取（settings_config 结构互不相同）=====

    /// OpenClaw: `{ baseUrl, apiKey, api, ... }`（camelCase）
    /// Codex / Grok Build: the endpoint lives in the embedded `config.toml`
    /// (`model_provider` → `model_providers.<id>.base_url`), not in an env map.
    fn extract_codex_base_url(provider: &Provider) -> Result<String, AppError> {
        if let Some(config) = provider.settings_config.get("config").and_then(|v| v.as_str()) {
            if let Ok(doc) = config.parse::<toml::Value>() {
                let provider_id = doc.get("model_provider").and_then(|v| v.as_str());
                if let Some(id) = provider_id {
                    if let Some(url) = doc
                        .get("model_providers")
                        .and_then(|m| m.get(id))
                        .and_then(|p| p.get("base_url"))
                        .and_then(|v| v.as_str())
                    {
                        let trimmed = url.trim();
                        if !trimmed.is_empty() {
                            return Ok(trimmed.to_string());
                        }
                    }
                }
                if let Some(table) = doc.get("model_providers").and_then(|v| v.as_table()) {
                    for entry in table.values() {
                        if let Some(url) = entry.get("base_url").and_then(|v| v.as_str()) {
                            let trimmed = url.trim();
                            if !trimmed.is_empty() {
                                return Ok(trimmed.to_string());
                            }
                        }
                    }
                }
                if let Some(url) = doc.get("openai_base_url").and_then(|v| v.as_str()) {
                    let trimmed = url.trim();
                    if !trimmed.is_empty() {
                        return Ok(trimmed.to_string());
                    }
                }
            }
        }
        Self::extract_env_base_url(provider, "codex")
    }

    /// Read the upstream base URL out of a provider row for the apps that
    /// keep it in an `env` map.
    fn extract_env_base_url(provider: &Provider, app: &str) -> Result<String, AppError> {
        let key = match app {
            "claude" => "ANTHROPIC_BASE_URL",
            "codex" | "grokbuild" => "OPENAI_BASE_URL",
            "gemini" => "GOOGLE_GEMINI_BASE_URL",
            other => {
                return Err(AppError::Message(format!("{other} 不支持连通性检测")));
            }
        };
        provider
            .settings_config
            .get("env")
            .and_then(|v| v.get(key))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .ok_or_else(|| AppError::Message(format!("供应商未配置 {key}，无法检测连通性")))
    }

    fn extract_openclaw_base_url(provider: &Provider) -> Result<String, AppError> {
        provider
            .settings_config
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AppError::localized(
                    "openclaw_base_url_missing",
                    "OpenClaw 供应商缺少 baseUrl",
                    "OpenClaw provider is missing `baseUrl`",
                )
            })
    }

    /// Hermes: `{ base_url, api_key, api_mode }`（snake_case）
    fn extract_hermes_base_url(provider: &Provider) -> Result<String, AppError> {
        provider
            .settings_config
            .get("base_url")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AppError::localized(
                    "hermes_base_url_missing",
                    "Hermes 供应商缺少 base_url",
                    "Hermes provider is missing `base_url`",
                )
            })
    }

    /// OpenCode: `{ npm, options: { baseURL, apiKey }, ... }`
    ///
    /// 用户未显式填 `options.baseURL` 时，按 `npm`（AI SDK 包）回退到包自带默认端点。
    /// `@ai-sdk/openai-compatible` 无默认端点，必须显式填。
    fn resolve_opencode_base_url(
        provider: &Provider,
        npm: Option<&str>,
    ) -> Result<String, AppError> {
        if let Some(explicit) = Self::extract_opencode_base_url(provider) {
            return Ok(explicit);
        }

        let fallback = match npm {
            Some("@ai-sdk/openai") => Some("https://api.openai.com/v1"),
            Some("@ai-sdk/anthropic") => Some("https://api.anthropic.com"),
            Some("@ai-sdk/google") => Some("https://generativelanguage.googleapis.com"),
            _ => None,
        };

        fallback.map(|s| s.to_string()).ok_or_else(|| {
            AppError::localized(
                "opencode_base_url_missing",
                "OpenCode 供应商缺少 options.baseURL，且当前 SDK 包没有默认端点",
                "OpenCode provider is missing `options.baseURL` and the SDK package has no default endpoint",
            )
        })
    }

    fn extract_opencode_base_url(provider: &Provider) -> Option<String> {
        provider
            .settings_config
            .get("options")
            .and_then(|v| v.get("baseURL"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn extract_opencode_npm(provider: &Provider) -> Option<String> {
        provider
            .settings_config
            .get("npm")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_provider(settings_config: serde_json::Value) -> Provider {
        Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            settings_config,
            None,
        )
    }
}
