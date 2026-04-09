//! Microbenchmarks for ID generation hot paths.
//!
//! WHY: ULID and UUID generation runs on every session/turn/observation
//! creation across the workspace. Tracking these here surfaces any
//! regression introduced by changes to the internal implementations
//! (`koina::ulid`, `koina::uuid`) — both are vendored from external
//! crates and should not regress beyond the original baseline.
//!
//! Run: `cargo bench -p aletheia-koina`
//! Run a single bench: `cargo bench -p aletheia-koina -- ulid_new`

#![expect(clippy::expect_used, reason = "bench setup")]

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use aletheia_koina::ulid::Ulid;
use aletheia_koina::uuid::{Uuid, uuid_v4};

fn ulid_new(c: &mut Criterion) {
    c.bench_function("ulid_new", |b| {
        b.iter(|| {
            let id = Ulid::new();
            black_box(id)
        });
    });
}

fn ulid_to_string(c: &mut Criterion) {
    let id = Ulid::new();
    c.bench_function("ulid_to_string", |b| {
        b.iter(|| {
            let s = black_box(id).to_string();
            black_box(s)
        });
    });
}

fn ulid_from_str(c: &mut Criterion) {
    let id = Ulid::new().to_string();
    c.bench_function("ulid_from_str", |b| {
        b.iter(|| {
            let parsed: Ulid = black_box(&id).parse().expect("valid ULID");
            black_box(parsed)
        });
    });
}

fn uuid_new_v4(c: &mut Criterion) {
    c.bench_function("uuid_new_v4", |b| {
        b.iter(|| {
            let id = Uuid::new_v4();
            black_box(id)
        });
    });
}

fn uuid_v4_string(c: &mut Criterion) {
    c.bench_function("uuid_v4_string", |b| {
        b.iter(|| {
            let s = uuid_v4();
            black_box(s)
        });
    });
}

fn uuid_parse_str(c: &mut Criterion) {
    let s = uuid_v4();
    c.bench_function("uuid_parse_str", |b| {
        b.iter(|| {
            let parsed = Uuid::parse_str(black_box(&s)).expect("valid UUID");
            black_box(parsed)
        });
    });
}

criterion_group!(
    benches,
    ulid_new,
    ulid_to_string,
    ulid_from_str,
    uuid_new_v4,
    uuid_v4_string,
    uuid_parse_str
);
criterion_main!(benches);
