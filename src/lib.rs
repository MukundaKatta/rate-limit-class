//! Parse rate-limit responses from LLM providers into a single shape.
//!
//! Each provider exposes rate-limit info differently:
//!
//! - **Anthropic** uses `anthropic-ratelimit-tokens-reset` / `-requests-reset`
//!   headers (ISO-8601 timestamps), plus the standard `retry-after`.
//! - **OpenAI** uses `x-ratelimit-reset-tokens` / `-requests` (seconds), plus
//!   `retry-after` and a JSON body with `error.code = rate_limit_exceeded`.
//! - **Google Gemini** uses `retry-after` and a JSON body with
//!   `error.status = "RESOURCE_EXHAUSTED"`.
//! - **AWS Bedrock** uses HTTP `ThrottlingException` with no retry-after;
//!   you back off on your own.
//!
//! `rate-limit-class` normalizes all of these into one [`RateLimitInfo`]
//! with `retry_after` as a `Duration`, plus `kind` (RPM / TPM / concurrent)
//! and the [`Provider`] it parsed.
//!
//! # Quick start
//!
//! ```
//! use rate_limit_class::{classify_anthropic, classify_openai, RateLimitKind};
//! use std::collections::HashMap;
//!
//! let mut h = HashMap::new();
//! h.insert("retry-after".into(), "30".into());
//! h.insert("anthropic-ratelimit-tokens-remaining".into(), "0".into());
//!
//! let info = classify_anthropic(429, &h, None).unwrap();
//! assert_eq!(info.retry_after.unwrap().as_secs(), 30);
//! assert_eq!(info.kind, Some(RateLimitKind::TokensPerMinute));
//! ```
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(rust_2018_idioms)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Which provider this rate-limit response came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    /// Anthropic API (`api.anthropic.com`).
    Anthropic,
    /// OpenAI API (`api.openai.com`).
    OpenAI,
    /// Google Gemini API.
    Gemini,
    /// AWS Bedrock runtime.
    Bedrock,
}

/// What kind of rate limit was hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RateLimitKind {
    /// Requests-per-minute cap.
    RequestsPerMinute,
    /// Tokens-per-minute cap.
    TokensPerMinute,
    /// Concurrent-request cap.
    Concurrent,
    /// Could not classify.
    Unknown,
}

/// Parsed rate-limit info, unified across providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitInfo {
    /// Best-guess `Retry-After` duration from headers or body.
    pub retry_after: Option<Duration>,
    /// Which kind of cap was hit (RPM / TPM / concurrent).
    pub kind: Option<RateLimitKind>,
    /// Which provider parsed this response.
    pub provider: Provider,
}

fn parse_retry_after(headers: &HashMap<String, String>) -> Option<Duration> {
    let value = lookup_ci(headers, "retry-after")?;
    if let Ok(secs) = value.parse::<f64>() {
        if secs >= 0.0 {
            return Some(Duration::from_secs_f64(secs));
        }
    }
    // HTTP-date case (RFC 7231) — we don't parse dates without a dep; users
    // can fall back to a default backoff if `None`.
    None
}

fn lookup_ci(headers: &HashMap<String, String>, key: &str) -> Option<String> {
    let k_lc = key.to_lowercase();
    for (h, v) in headers {
        if h.to_lowercase() == k_lc {
            return Some(v.clone());
        }
    }
    None
}

/// Classify an Anthropic 429 response.
pub fn classify_anthropic(
    status: u16,
    headers: &HashMap<String, String>,
    _body: Option<&str>,
) -> Option<RateLimitInfo> {
    if status != 429 {
        return None;
    }
    let retry_after = parse_retry_after(headers);

    let kind = if lookup_ci(headers, "anthropic-ratelimit-tokens-remaining")
        .map(|v| v == "0")
        .unwrap_or(false)
    {
        Some(RateLimitKind::TokensPerMinute)
    } else if lookup_ci(headers, "anthropic-ratelimit-requests-remaining")
        .map(|v| v == "0")
        .unwrap_or(false)
    {
        Some(RateLimitKind::RequestsPerMinute)
    } else {
        Some(RateLimitKind::Unknown)
    };

    Some(RateLimitInfo {
        retry_after,
        kind,
        provider: Provider::Anthropic,
    })
}

/// Classify an OpenAI 429 response.
pub fn classify_openai(
    status: u16,
    headers: &HashMap<String, String>,
    body: Option<&str>,
) -> Option<RateLimitInfo> {
    if status != 429 {
        return None;
    }
    let retry_after = parse_retry_after(headers);

    // OpenAI hints whether it was a TPM or RPM hit via the remaining headers
    // being zero. If the body says "tokens per min", trust that over headers.
    let body_hint = body.map(|b| b.to_lowercase());
    let kind = if body_hint
        .as_ref()
        .map(|b| b.contains("tokens per min") || b.contains("tpm"))
        .unwrap_or(false)
    {
        Some(RateLimitKind::TokensPerMinute)
    } else if body_hint
        .as_ref()
        .map(|b| b.contains("requests per min") || b.contains("rpm"))
        .unwrap_or(false)
    {
        Some(RateLimitKind::RequestsPerMinute)
    } else if lookup_ci(headers, "x-ratelimit-remaining-tokens")
        .map(|v| v == "0")
        .unwrap_or(false)
    {
        Some(RateLimitKind::TokensPerMinute)
    } else if lookup_ci(headers, "x-ratelimit-remaining-requests")
        .map(|v| v == "0")
        .unwrap_or(false)
    {
        Some(RateLimitKind::RequestsPerMinute)
    } else {
        Some(RateLimitKind::Unknown)
    };

    Some(RateLimitInfo {
        retry_after,
        kind,
        provider: Provider::OpenAI,
    })
}

/// Classify a Google Gemini 429 / `RESOURCE_EXHAUSTED` response.
pub fn classify_gemini(
    status: u16,
    headers: &HashMap<String, String>,
    body: Option<&str>,
) -> Option<RateLimitInfo> {
    let is_429 = status == 429;
    let is_resource_exhausted = body
        .map(|b| b.contains("RESOURCE_EXHAUSTED"))
        .unwrap_or(false);
    if !(is_429 || is_resource_exhausted) {
        return None;
    }
    Some(RateLimitInfo {
        retry_after: parse_retry_after(headers),
        kind: Some(RateLimitKind::Unknown),
        provider: Provider::Gemini,
    })
}

/// Classify an AWS Bedrock `ThrottlingException`. No retry-after on Bedrock;
/// caller must back off using its own policy.
pub fn classify_bedrock(
    status: u16,
    headers: &HashMap<String, String>,
    body: Option<&str>,
) -> Option<RateLimitInfo> {
    let is_429 = status == 429;
    let is_throttle = body
        .map(|b| b.contains("ThrottlingException") || b.contains("TooManyRequestsException"))
        .unwrap_or(false);
    if !(is_429 || is_throttle) {
        return None;
    }
    Some(RateLimitInfo {
        retry_after: parse_retry_after(headers),
        kind: Some(RateLimitKind::Unknown),
        provider: Provider::Bedrock,
    })
}

/// Auto-detect provider from headers and dispatch.
pub fn classify_headers(
    status: u16,
    headers: &HashMap<String, String>,
    body: Option<&str>,
) -> Option<RateLimitInfo> {
    if headers.keys().any(|k| {
        let k = k.to_lowercase();
        k.starts_with("anthropic-")
    }) {
        return classify_anthropic(status, headers, body);
    }
    if headers.keys().any(|k| {
        let k = k.to_lowercase();
        k.starts_with("x-ratelimit-") || k == "openai-organization"
    }) {
        return classify_openai(status, headers, body);
    }
    if body
        .map(|b| b.contains("RESOURCE_EXHAUSTED"))
        .unwrap_or(false)
    {
        return classify_gemini(status, headers, body);
    }
    if body
        .map(|b| b.contains("ThrottlingException") || b.contains("TooManyRequestsException"))
        .unwrap_or(false)
    {
        return classify_bedrock(status, headers, body);
    }
    None
}
