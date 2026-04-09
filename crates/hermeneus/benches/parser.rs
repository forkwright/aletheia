//! Microbenchmarks for hermeneus hot paths reachable from the public API.
//!
//! WHY: the SSE parser (`parse_sse_stream`) is the hottest path for every
//! streamed completion, but it lives in a `pub(crate)` module gated behind
//! `cfg(test)`, so a `benches/` binary cannot reach it without a visibility
//! change that is out of scope for the #2802 audit. Instead we bench the
//! next layer of hot-path surface that _is_ public:
//!
//!   * [`Usage::total`] — trivial arithmetic baseline; a regression here
//!     would mean something very strange is happening with inlining.
//!   * [`StopReason::as_str`] and `FromStr` — wire-format round trip on
//!     every completion response.
//!   * [`ToolResultType::classify`] — string classification on every tool
//!     result, used by the compaction TTL pass.
//!   * [`complexity::score_complexity`] — regex-heavy scoring that runs on
//!     every user message before routing to a model tier.
//!   * [`AdaptiveConcurrencyLimiter::acquire`] + `ConcurrencyPermit::finish`
//!     — the uncontended fast path wraps every provider call.
//!
//! Run: `cargo bench -p aletheia-hermeneus`
//! Filter: `cargo bench -p aletheia-hermeneus -- classify_tool_name`

#![expect(clippy::expect_used, reason = "bench setup; fatal on fixture failure")]

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use aletheia_hermeneus::complexity::{ComplexityInput, score_complexity};
use aletheia_hermeneus::concurrency::{
    AdaptiveConcurrencyLimiter, ConcurrencyConfig, RequestOutcome,
};
use aletheia_hermeneus::types::{StopReason, ToolResultType, Usage};

/// Baseline: sum two `u64` fields inside a `Copy` struct. Establishes the
/// noise floor for the benchmark harness on this host.
fn usage_total(c: &mut Criterion) {
    let usage = Usage {
        input_tokens: 12_345,
        output_tokens: 6_789,
        cache_read_tokens: 200,
        cache_write_tokens: 100,
    };
    c.bench_function("usage_total", |b| {
        b.iter(|| {
            let total = black_box(&usage).total();
            black_box(total)
        });
    });
}

/// `StopReason::as_str` is called on every completion response when
/// serializing wire events back out. It is a one-branch match, but
/// tracking it catches inlining regressions.
fn stop_reason_as_str(c: &mut Criterion) {
    let reasons = [
        StopReason::EndTurn,
        StopReason::ToolUse,
        StopReason::MaxTokens,
        StopReason::StopSequence,
    ];
    c.bench_function("stop_reason_as_str", |b| {
        b.iter(|| {
            let mut last = "";
            for r in black_box(&reasons) {
                last = r.as_str();
            }
            black_box(last)
        });
    });
}

/// `StopReason::from_str` is called on every deserialized completion
/// response. Regressions would hit every turn.
fn stop_reason_from_str(c: &mut Criterion) {
    let inputs = ["end_turn", "tool_use", "max_tokens", "stop_sequence"];
    c.bench_function("stop_reason_from_str", |b| {
        b.iter(|| {
            for s in black_box(&inputs) {
                let parsed = StopReason::from_str(black_box(s)).expect("known variant");
                black_box(parsed);
            }
        });
    });
}

/// `ToolResultType::classify` runs on every tool result during the
/// compaction pass to pick a TTL. It lowercases and substring-matches
/// against a handful of keywords, so it is short but not free.
fn classify_tool_name(c: &mut Criterion) {
    // WHY: cover each branch of the classifier so the benchmark is not
    // biased toward the first match — Read (file), Bash (shell), WebSearch
    // (web), Grep (search), Unknown (other).
    let names = ["Read", "Bash", "WebSearch", "Grep", "Unknown"];
    c.bench_function("classify_tool_name", |b| {
        b.iter(|| {
            for name in black_box(&names) {
                let kind = ToolResultType::classify(black_box(name));
                black_box(kind);
            }
        });
    });
}

/// `score_complexity` runs on every user message when complexity-based
/// routing is enabled. It compiles a dozen regexes once via `LazyLock` and
/// then runs each against the message text, so this bench captures the
/// steady-state cost after warmup.
fn score_complexity_short(c: &mut Criterion) {
    let input = ComplexityInput {
        message_text: "quick question: what's the capital of France?",
        tool_count: 2,
        message_count: 1,
        depth: 0,
        tier_override: None,
        model_override: None,
    };
    c.bench_function("score_complexity_short", |b| {
        b.iter(|| {
            let score = score_complexity(black_box(&input));
            black_box(score)
        });
    });
}

/// Longer, more representative message with multiple regex hits. This is
/// closer to the realistic cost of routing a typical assistant turn.
fn score_complexity_long(c: &mut Criterion) {
    let input = ComplexityInput {
        message_text:
            "Please analyze the architecture of this service, then implement a refactor \
             that migrates the session store to the new backend. Review the diff \
             and investigate any failing tests. After that, commit and push.",
        tool_count: 12,
        message_count: 6,
        depth: 0,
        tier_override: None,
        model_override: None,
    };
    c.bench_function("score_complexity_long", |b| {
        b.iter(|| {
            let score = score_complexity(black_box(&input));
            black_box(score)
        });
    });
}

/// Uncontended acquire + finish cycle on the AIMD limiter.
///
/// WHY: every LLM call goes through this path. The fast path is a single
/// `Mutex` lock, an `in_flight` increment, and on `finish` a second lock
/// to apply the EWMA update — `acquire` is `async`, so the benchmark
/// drives it on a current-thread runtime to isolate the limiter logic
/// from any executor overhead. Because the permit is never blocked in
/// the single-worker case, no actual tokio scheduling happens.
fn concurrency_acquire_release(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("tokio current-thread runtime");
    let limiter = Arc::new(AdaptiveConcurrencyLimiter::new(
        "bench",
        ConcurrencyConfig {
            // WHY: a large max prevents the limit from saturating during a
            // long benchmark run and skewing numbers toward the min-limit
            // floor as the successive "Success" outcomes push it up.
            initial_limit: 1_024,
            min_limit: 1,
            max_limit: 1_024,
            increase_step: 1,
            decrease_factor: 0.9,
            ewma_alpha: 0.8,
            latency_threshold_secs: 30.0,
        },
    ));

    c.bench_function("concurrency_acquire_release", |b| {
        b.iter(|| {
            rt.block_on(async {
                let permit = limiter.acquire().await;
                permit.finish_with_latency(
                    RequestOutcome::Neutral,
                    Duration::from_millis(1),
                );
            });
            black_box(&limiter);
        });
    });
}

criterion_group!(
    benches,
    usage_total,
    stop_reason_as_str,
    stop_reason_from_str,
    classify_tool_name,
    score_complexity_short,
    score_complexity_long,
    concurrency_acquire_release
);
criterion_main!(benches);
