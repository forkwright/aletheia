//! Microbenchmarks for episteme observation extraction hot paths.
//!
//! WHY: `parse_observations`, `extract_tags`, and `ObservationType::classify`
//! all run on every PR scraped by the bookkeeper, which can be hundreds per
//! day. They're pure string parsers and should stay sub-microsecond per call.
//!
//! Run: `cargo bench -p aletheia-episteme --bench observation`

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use episteme::extract::observation::{
    ObservationType, extract_tags, parse_observations,
};

const SHORT_BODY: &str = "## Observations\n- bug: foo crashed when bar #tag\n- idea: try baz instead";

const LONG_BODY: &str = "
## Summary
This PR fixes a thing.

## Observations
- bug: division by zero in scoring logic when no candidates exist #scoring #bug
- debt: the validation function is duplicated across three modules #debt #refactor
- idea: cache the rule lookup result; it's called per-token #perf
- doc_gap: missing doc on `compute_score` #docs
- missing_test: no test for empty input edge case #testing

## Test plan
- unit tests
";

fn parse_observations_short(c: &mut Criterion) {
    c.bench_function("parse_observations_short", |b| {
        b.iter(|| {
            let obs = parse_observations(black_box(SHORT_BODY));
            black_box(obs)
        });
    });
}

fn parse_observations_long(c: &mut Criterion) {
    c.bench_function("parse_observations_long", |b| {
        b.iter(|| {
            let obs = parse_observations(black_box(LONG_BODY));
            black_box(obs)
        });
    });
}

fn extract_tags_simple(c: &mut Criterion) {
    let text = "this is a test #foo #bar #baz with several tags";
    c.bench_function("extract_tags_simple", |b| {
        b.iter(|| {
            let tags = extract_tags(black_box(text));
            black_box(tags)
        });
    });
}

fn classify_observation_type(c: &mut Criterion) {
    let bug = "bug: foo crashed when bar";
    let idea = "idea: try baz instead";
    let debt = "debt: the validation function is duplicated";
    c.bench_function("classify_observation_type", |b| {
        b.iter(|| {
            let _ = ObservationType::classify(black_box(bug));
            let _ = ObservationType::classify(black_box(idea));
            let _ = ObservationType::classify(black_box(debt));
        });
    });
}

criterion_group!(
    benches,
    parse_observations_short,
    parse_observations_long,
    extract_tags_simple,
    classify_observation_type,
);
criterion_main!(benches);
