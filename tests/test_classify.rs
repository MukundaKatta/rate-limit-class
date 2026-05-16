use rate_limit_class::{
    classify_anthropic, classify_bedrock, classify_gemini, classify_headers, classify_openai,
    Provider, RateLimitKind,
};
use std::collections::HashMap;

fn h(items: &[(&str, &str)]) -> HashMap<String, String> {
    items.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn anthropic_tpm_hit() {
    let headers = h(&[
        ("retry-after", "30"),
        ("anthropic-ratelimit-tokens-remaining", "0"),
    ]);
    let info = classify_anthropic(429, &headers, None).unwrap();
    assert_eq!(info.provider, Provider::Anthropic);
    assert_eq!(info.kind, Some(RateLimitKind::TokensPerMinute));
    assert_eq!(info.retry_after.unwrap().as_secs(), 30);
}

#[test]
fn anthropic_rpm_hit() {
    let headers = h(&[
        ("retry-after", "5"),
        ("anthropic-ratelimit-requests-remaining", "0"),
    ]);
    let info = classify_anthropic(429, &headers, None).unwrap();
    assert_eq!(info.kind, Some(RateLimitKind::RequestsPerMinute));
}

#[test]
fn anthropic_non_429_returns_none() {
    let info = classify_anthropic(200, &HashMap::new(), None);
    assert!(info.is_none());
}

#[test]
fn openai_tpm_from_body() {
    let body = r#"{"error":{"message":"Rate limit reached: 90000 tokens per min","code":"rate_limit_exceeded"}}"#;
    let info = classify_openai(429, &HashMap::new(), Some(body)).unwrap();
    assert_eq!(info.kind, Some(RateLimitKind::TokensPerMinute));
    assert_eq!(info.provider, Provider::OpenAI);
}

#[test]
fn openai_rpm_from_body() {
    let body = r#"{"error":{"message":"Rate limit reached: 500 requests per min"}}"#;
    let info = classify_openai(429, &HashMap::new(), Some(body)).unwrap();
    assert_eq!(info.kind, Some(RateLimitKind::RequestsPerMinute));
}

#[test]
fn openai_kind_from_headers_fallback() {
    let headers = h(&[
        ("retry-after", "1"),
        ("x-ratelimit-remaining-tokens", "0"),
    ]);
    let info = classify_openai(429, &headers, None).unwrap();
    assert_eq!(info.kind, Some(RateLimitKind::TokensPerMinute));
}

#[test]
fn gemini_resource_exhausted_in_body() {
    let body = r#"{"error":{"code":429,"message":"Quota exceeded","status":"RESOURCE_EXHAUSTED"}}"#;
    // Even with a 200 status (some Gemini routes return 200 + body error)
    let info = classify_gemini(200, &HashMap::new(), Some(body)).unwrap();
    assert_eq!(info.provider, Provider::Gemini);
}

#[test]
fn bedrock_throttling_exception() {
    let body = r#"{"__type":"ThrottlingException","message":"Too many tokens"}"#;
    let info = classify_bedrock(400, &HashMap::new(), Some(body)).unwrap();
    assert_eq!(info.provider, Provider::Bedrock);
    assert!(info.retry_after.is_none()); // Bedrock doesn't send retry-after
}

#[test]
fn dispatch_picks_anthropic_via_header_prefix() {
    let headers = h(&[
        ("anthropic-ratelimit-tokens-remaining", "0"),
        ("retry-after", "10"),
    ]);
    let info = classify_headers(429, &headers, None).unwrap();
    assert_eq!(info.provider, Provider::Anthropic);
}

#[test]
fn dispatch_picks_openai_via_header_prefix() {
    let headers = h(&[("x-ratelimit-remaining-requests", "0"), ("retry-after", "2")]);
    let info = classify_headers(429, &headers, None).unwrap();
    assert_eq!(info.provider, Provider::OpenAI);
}

#[test]
fn dispatch_picks_gemini_via_body() {
    let body = r#"{"error":{"status":"RESOURCE_EXHAUSTED"}}"#;
    let info = classify_headers(429, &HashMap::new(), Some(body)).unwrap();
    assert_eq!(info.provider, Provider::Gemini);
}

#[test]
fn dispatch_picks_bedrock_via_body() {
    let body = r#"{"__type":"ThrottlingException"}"#;
    let info = classify_headers(400, &HashMap::new(), Some(body)).unwrap();
    assert_eq!(info.provider, Provider::Bedrock);
}

#[test]
fn dispatch_returns_none_for_unrecognized() {
    let info = classify_headers(429, &HashMap::new(), None);
    assert!(info.is_none());
}

#[test]
fn retry_after_handles_fractional_seconds() {
    let headers = h(&[("retry-after", "0.5"), ("anthropic-ratelimit-tokens-remaining", "0")]);
    let info = classify_anthropic(429, &headers, None).unwrap();
    assert_eq!(info.retry_after.unwrap().as_millis(), 500);
}

#[test]
fn case_insensitive_headers() {
    let headers = h(&[
        ("Retry-After", "15"),
        ("Anthropic-Ratelimit-Tokens-Remaining", "0"),
    ]);
    let info = classify_anthropic(429, &headers, None).unwrap();
    assert_eq!(info.retry_after.unwrap().as_secs(), 15);
    assert_eq!(info.kind, Some(RateLimitKind::TokensPerMinute));
}
