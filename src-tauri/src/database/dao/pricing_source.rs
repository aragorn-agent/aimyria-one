//! Pricing-source constants, extracted from upstream's `database/dao/proxy.rs`.
//!
//! Upstream keeps these at the top of `dao/proxy.rs`, whose remaining 700-odd
//! lines store per-request proxy logs and need `crate::proxy::types`. This build
//! runs no proxy, so that file is not copied — but `proxy/usage/logger.rs` still
//! writes request logs and has to label each row as priced from the request
//! payload or from the response.
//!
//! Lines 10-24 of upstream's `dao/proxy.rs`, verbatim. The names and values are
//! shared with the `pricing_source` column, so they are part of the on-disk
//! format rather than an internal detail.

use crate::error::AppError;

pub(crate) const PRICING_SOURCE_RESPONSE: &str = "response";
pub(crate) const PRICING_SOURCE_REQUEST: &str = "request";

pub(crate) fn validate_pricing_source(value: &str) -> Result<&str, AppError> {
    let trimmed = value.trim();
    if trimmed == PRICING_SOURCE_RESPONSE || trimmed == PRICING_SOURCE_REQUEST {
        Ok(trimmed)
    } else {
        Err(AppError::localized(
            "error.invalidPricingMode",
            format!("无效计费模式: {value}"),
            format!("Invalid pricing mode: {value}"),
        ))
    }
}
