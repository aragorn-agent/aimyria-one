//! SQL fragment helpers shared across usage aggregation queries.
//!
//! Anthropic reports `input_tokens` as fresh (cache reads counted
//! separately); OpenAI Responses API and Google Gemini's
//! `promptTokenCount` both include the cached portion. Any aggregation
//! summing `input_tokens` across providers must route through
//! [`fresh_input_sql`] to recover a consistent semantics.

/// Set of `app_type` values whose stored `input_tokens` already includes
/// `cache_read_tokens`. Aggregations subtract cache reads from these rows
/// to recover the fresh-input semantics used by Claude.
///
/// Why list providers explicitly: new providers default to the
/// Claude-style "input excludes cache" semantics, which is safer if the
/// caller forgets to update this list. The wrong direction (a new OpenAI-
/// style provider not added here) shows up loudly as a too-low cache hit
/// rate, which is easier to catch than the silent over-deduction that
/// would happen with the opposite default.
/// 单一语义集（SSOT）：写入侧（proxy logger/calculator）、回填侧
/// （usage_stats 成本重算）与展示侧（本文件的 SQL 归一）都必须引用这里，
/// 防止同一语义散落多处后新增 app 时漏改（grokbuild 曾在回填侧漏掉）。
/// 前端 `src/types/usage.ts` 的同名常量是跨语言的对应物，改动须同步。
pub(crate) const CACHE_INCLUSIVE_APP_TYPES: &[&str] = &["codex", "gemini", "grokbuild"];

/// `app_type` 的存储 `input_tokens` 是否已包含 cache read/write。
pub(crate) fn is_cache_inclusive_app(app_type: &str) -> bool {
    CACHE_INCLUSIVE_APP_TYPES.contains(&app_type)
}

pub(crate) const INPUT_TOKEN_SEMANTICS_LEGACY: i64 = 0;
pub(crate) const INPUT_TOKEN_SEMANTICS_TOTAL: i64 = 1;
pub(crate) const INPUT_TOKEN_SEMANTICS_FRESH: i64 = 2;

/// Build an SQL expression that returns the cache-normalized `input_tokens`
/// for a single row in `proxy_request_logs` or `usage_daily_rollups`.
///
/// Legacy rows subtract cache reads only. New total-inclusive rows subtract
/// both cache reads and writes. Rollups normalized to fresh input are returned
/// unchanged.
///
/// Pass an empty string to reference the columns directly (no alias),
/// or a table alias such as `"l"` to emit `l.input_tokens` style references.
pub fn fresh_input_sql(alias: &str) -> String {
    let prefix = if alias.is_empty() {
        String::new()
    } else {
        format!("{alias}.")
    };
    let app_type_list = CACHE_INCLUSIVE_APP_TYPES
        .iter()
        .map(|t| format!("'{t}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "CASE \
              WHEN {prefix}input_token_semantics = {INPUT_TOKEN_SEMANTICS_FRESH} THEN {prefix}input_tokens \
              WHEN {prefix}app_type IN ({app_type_list}) \
                   AND {prefix}input_token_semantics = {INPUT_TOKEN_SEMANTICS_TOTAL} \
                   AND {prefix}input_tokens >= ({prefix}cache_read_tokens + {prefix}cache_creation_tokens) \
              THEN ({prefix}input_tokens - {prefix}cache_read_tokens - {prefix}cache_creation_tokens) \
              WHEN {prefix}app_type IN ({app_type_list}) \
                   AND {prefix}input_token_semantics = {INPUT_TOKEN_SEMANTICS_LEGACY} \
                   AND {prefix}input_tokens >= {prefix}cache_read_tokens \
              THEN ({prefix}input_tokens - {prefix}cache_read_tokens) \
              ELSE {prefix}input_tokens END"
    )
}

/// 单行的「真实消耗 Tokens」：新增输入 + 输出 + 缓存写入 + 缓存命中。
///
/// 与 `usage_stats::derive_real_total_and_hit_rate`（指标卡 / 趋势图）同口径，
/// 供应商、模型等分组统计的 `total_tokens` 都用它，保证各行之和等于顶部总数。
pub fn real_total_tokens_sql(alias: &str) -> String {
    let prefix = if alias.is_empty() {
        String::new()
    } else {
        format!("{alias}.")
    };
    let fresh_input = fresh_input_sql(alias);
    format!(
        "(({fresh_input}) + {prefix}output_tokens \
          + {prefix}cache_creation_tokens + {prefix}cache_read_tokens)"
    )
}
