//! Microbenchmarks for nous token budget hot paths.
//!
//! WHY: `CharEstimator::estimate` runs on every system prompt assembly,
//! every distillation candidate selection, and every history truncation.
//! `TokenBudget` arithmetic runs on every turn boundary. These should
//! stay nanoseconds-per-call.
//!
//! Run: `cargo bench -p aletheia-nous --bench budget`

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use aletheia_nous::budget::{CharEstimator, TokenBudget, TokenEstimator};

const SHORT_TEXT: &str = "Hello, world!";
const MEDIUM_TEXT: &str = "The quick brown fox jumps over the lazy dog. \
This is a moderately sized passage that exercises the character-based \
estimator at a realistic conversational scale.";
const LONG_TEXT: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim \
ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip \
ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate \
velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat \
cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id \
est laborum. \
At vero eos et accusamus et iusto odio dignissimos ducimus qui blanditiis \
praesentium voluptatum deleniti atque corrupti quos dolores et quas molestias \
excepturi sint occaecati cupiditate non provident, similique sunt in culpa \
qui officia deserunt mollitia animi, id est laborum et dolorum fuga.";

fn estimate_short(c: &mut Criterion) {
    let estimator = CharEstimator::new(4);
    c.bench_function("char_estimator_short", |b| {
        b.iter(|| {
            let n = estimator.estimate(black_box(SHORT_TEXT));
            black_box(n)
        });
    });
}

fn estimate_medium(c: &mut Criterion) {
    let estimator = CharEstimator::new(4);
    c.bench_function("char_estimator_medium", |b| {
        b.iter(|| {
            let n = estimator.estimate(black_box(MEDIUM_TEXT));
            black_box(n)
        });
    });
}

fn estimate_long(c: &mut Criterion) {
    let estimator = CharEstimator::new(4);
    c.bench_function("char_estimator_long", |b| {
        b.iter(|| {
            let n = estimator.estimate(black_box(LONG_TEXT));
            black_box(n)
        });
    });
}

fn budget_new(c: &mut Criterion) {
    c.bench_function("token_budget_new", |b| {
        b.iter(|| {
            let budget =
                TokenBudget::new(black_box(200_000), black_box(0.6), black_box(8_192), black_box(40_000));
            black_box(budget)
        });
    });
}

criterion_group!(
    benches,
    estimate_short,
    estimate_medium,
    estimate_long,
    budget_new,
);
criterion_main!(benches);
