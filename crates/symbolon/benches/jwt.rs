//! Microbenchmarks for symbolon JWT hot paths.
//!
//! WHY: Every authenticated request through pylon hits `JwtManager::validate`,
//! which does HMAC verification + JSON parse + claim validation. Track its
//! cost so latency regressions surface in CI rather than production.
//!
//! Run: `cargo bench -p aletheia-symbolon --bench jwt`

#![expect(clippy::expect_used, reason = "bench setup")]

use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};

use koina::secret::SecretString;
use symbolon::jwt::{JwtConfig, JwtManager};
use symbolon::types::Role;

fn make_manager() -> JwtManager {
    JwtManager::new(JwtConfig {
        signing_key: SecretString::from("bench-signing-key-with-enough-bytes".to_owned()),
        access_ttl: Duration::from_secs(3600),
        refresh_ttl: Duration::from_secs(86400),
        issuer: "bench".to_owned(),
    })
}

fn jwt_issue_access(c: &mut Criterion) {
    let manager = make_manager();
    c.bench_function("jwt_issue_access", |b| {
        b.iter(|| {
            let token = manager
                .issue_access(black_box("alice"), Role::Operator, None)
                .expect("issue access");
            black_box(token)
        });
    });
}

fn jwt_issue_access_with_nous_scope(c: &mut Criterion) {
    let manager = make_manager();
    c.bench_function("jwt_issue_access_with_nous_scope", |b| {
        b.iter(|| {
            let token = manager
                .issue_access(black_box("alice"), Role::Operator, Some(black_box("syn")))
                .expect("issue access");
            black_box(token)
        });
    });
}

fn jwt_validate_round_trip(c: &mut Criterion) {
    let manager = make_manager();
    let token = manager
        .issue_access("alice", Role::Operator, None)
        .expect("issue access");
    c.bench_function("jwt_validate_round_trip", |b| {
        b.iter(|| {
            let claims = manager.validate(black_box(&token)).expect("validate");
            black_box(claims)
        });
    });
}

criterion_group!(
    benches,
    jwt_issue_access,
    jwt_issue_access_with_nous_scope,
    jwt_validate_round_trip,
);
criterion_main!(benches);
