# rate-limit-class

[![CI](https://github.com/MukundaKatta/rate-limit-class/actions/workflows/ci.yml/badge.svg)](https://github.com/MukundaKatta/rate-limit-class/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/rate-limit-class.svg)](https://crates.io/crates/rate-limit-class)
[![docs.rs](https://docs.rs/rate-limit-class/badge.svg)](https://docs.rs/rate-limit-class)
[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

Parse rate-limit / 429 responses from Anthropic, OpenAI, Google Gemini, and AWS Bedrock into one unified shape.

```toml
[dependencies]
rate-limit-class = "0.1"
```

## Why

Every LLM provider exposes rate-limit info differently:

| Provider | How it tells you |
|---|---|
| Anthropic | `anthropic-ratelimit-{tokens,requests}-remaining` headers, plus `retry-after` |
| OpenAI | `x-ratelimit-remaining-{tokens,requests}` headers + `retry-after` + body message |
| Google Gemini | `retry-after` + body `error.status = RESOURCE_EXHAUSTED` |
| AWS Bedrock | `ThrottlingException` in body; no `retry-after`; you back off yourself |

You shouldn't have to write four `if` branches in every backoff handler. `rate-limit-class` is one function:

```rust
let info = classify_headers(status, &headers, body);
// info.retry_after — Duration
// info.kind        — RPM / TPM / concurrent
// info.provider    — Anthropic / OpenAI / Gemini / Bedrock
```

## Quick start

```rust
use rate_limit_class::{classify_anthropic, RateLimitKind};
use std::collections::HashMap;

let mut headers = HashMap::new();
headers.insert("retry-after".into(), "30".into());
headers.insert("anthropic-ratelimit-tokens-remaining".into(), "0".into());

let info = classify_anthropic(429, &headers, None).unwrap();
assert_eq!(info.retry_after.unwrap().as_secs(), 30);
assert_eq!(info.kind, Some(RateLimitKind::TokensPerMinute));

tokio::time::sleep(info.retry_after.unwrap()).await;
```

## Auto-detect

If you don't know which provider returned the response (e.g. a middleware
seeing arbitrary outbound traffic), use `classify_headers`:

```rust
use rate_limit_class::classify_headers;

let info = classify_headers(status, &headers, body);  // returns Option<RateLimitInfo>
```

Dispatch order:

1. Anthropic header prefix (`anthropic-*`)
2. OpenAI header prefix (`x-ratelimit-*`, `openai-organization`)
3. Gemini body (`RESOURCE_EXHAUSTED`)
4. Bedrock body (`ThrottlingException`)

## Composes with

- [`llm-error-class`](https://crates.io/crates/llm-error-class) — the broader error classifier across all error types. `rate-limit-class` is the narrower-but-deeper 429 specialist.
- [`agent-watchdog`](https://crates.io/crates/agent-watchdog) — feed `info.retry_after` into `watchdog.time_remaining()` to decide whether to wait or abort.
- [`agentidemp`](https://crates.io/crates/agentidemp) — re-use the same idempotency key when retrying after rate-limit so the provider dedups.

## What it doesn't do

- It doesn't parse HTTP-date `retry-after` values (`Retry-After: Wed, 21 Oct 2026 07:28:00 GMT`). All major LLM providers use integer-seconds; if you hit a date, `retry_after` returns `None`. Open an issue if you find a provider that sends dates.
- It doesn't sleep. You combine with `tokio::time::sleep` or your own backoff lib.
- It doesn't fire retries automatically. Wraps the *information*; the *policy* is yours.

## License

MIT
