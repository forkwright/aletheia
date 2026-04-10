//! Microbenchmarks for the `SessionStore` `SQLite` hot paths.
//!
//! WHY: every turn creates or finds a session and appends ≥2 messages.
//! These benchmarks track end-to-end `SQLite` cost (including the
//! `INSERT...SELECT MAX(seq)` pattern in `append_message`) so any
//! schema/index/prepared-statement change surfaces in CI.
//!
//! Run: `cargo bench -p aletheia-graphe`
//! Filter: `cargo bench -p aletheia-graphe -- create_session`
//!
//! Each bench uses an in-memory database to isolate from disk I/O —
//! this measures the `SQLite` engine + our SQL, not the storage stack.

#![expect(clippy::expect_used, reason = "bench setup")]

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use graphe::store::SessionStore;
use graphe::types::Role;
use koina::ulid::Ulid;

/// Build an in-memory store for one iteration's worth of work.
fn fresh_store() -> SessionStore {
    SessionStore::open_in_memory().expect("in-memory store opens")
}

/// `create_session` is on every new conversation. Cost is dominated by
/// the INSERT plus the follow-up SELECT used to return the row.
///
/// WHY: each iteration uses a unique `(nous_id, session_key)` pair to
/// avoid the UNIQUE constraint on sessions; running with `iter_batched`
/// would amortize that but adds non-load timing — the per-iter ULID
/// allocation is part of what real callers do and is fair to include.
fn create_session(c: &mut Criterion) {
    let store = fresh_store();
    let mut counter = 0_u64;
    c.bench_function("create_session", |b| {
        b.iter(|| {
            counter += 1;
            let id = Ulid::new().to_string();
            let session_key = format!("session-{counter}");
            let session = store
                .create_session(
                    black_box(&id),
                    black_box("nous-bench"),
                    black_box(&session_key),
                    black_box(None),
                    black_box(Some("claude-sonnet-4-6")),
                )
                .expect("session create");
            black_box(session);
        });
    });
}

/// `append_message` is on every user/assistant/tool turn. Cost includes
/// the seq-computation INSERT, the seq lookup, the session metadata
/// UPDATE, and a transaction commit.
fn append_message(c: &mut Criterion) {
    let store = fresh_store();
    let session_id = Ulid::new().to_string();
    store
        .create_session(&session_id, "nous-bench", "primary", None, None)
        .expect("session create");

    c.bench_function("append_message", |b| {
        b.iter(|| {
            let seq = store
                .append_message(
                    black_box(&session_id),
                    black_box(Role::User),
                    black_box("hello world"),
                    black_box(None),
                    black_box(None),
                    black_box(3),
                )
                .expect("append");
            black_box(seq);
        });
    });
}

/// `find_session_by_id` is on every dispatcher path that resumes a
/// session by primary key. The query is a single primary-key lookup;
/// this measures the round-trip overhead.
fn find_session_by_id(c: &mut Criterion) {
    let store = fresh_store();
    let session_id = Ulid::new().to_string();
    store
        .create_session(&session_id, "nous-bench", "primary", None, None)
        .expect("session create");

    c.bench_function("find_session_by_id", |b| {
        b.iter(|| {
            let session = store
                .find_session_by_id(black_box(&session_id))
                .expect("query");
            black_box(session);
        });
    });
}

criterion_group!(benches, create_session, append_message, find_session_by_id);
criterion_main!(benches);
