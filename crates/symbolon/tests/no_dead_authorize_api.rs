//! Regression guard for issue #7229.
//!
//! WHY (#7229): `Action` and `AuthFacade::authorize` were pylon's original
//! authorization primitive, superseded by the `Claims`/`require_role`/
//! `require_nous_access` model every handler uses today. #7227 removed the
//! last production caller (`require_credential_operator`), leaving `Action`
//! and `authorize()` as dead code exercised only by their own unit tests --
//! exactly the shape of a "second capability model" that looks load-bearing
//! to the next reader who reaches for it instead of `require_role`.
//!
//! This test asserts the dead surface stays removed by reading this crate's
//! own source (never another crate's) as data, the same pattern used by
//! `koina`'s `leaf_dependency_boundary` test.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::fs;

#[test]
fn types_no_longer_defines_dead_action_enum() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/types.rs");
    let source = fs::read_to_string(path).expect("types.rs is readable");

    assert!(
        !source.contains("pub enum Action {"),
        "types.rs must not reintroduce the dead `Action` enum removed in #7229 -- \
         it had zero production callers after #7227; restore a real caller \
         instead of resurrecting unused RBAC surface."
    );
}

#[test]
fn auth_no_longer_defines_dead_authorize_method() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/auth.rs");
    let source = fs::read_to_string(path).expect("auth.rs is readable");

    assert!(
        !source.contains("pub fn authorize("),
        "auth.rs must not reintroduce the dead `AuthFacade::authorize` method \
         removed in #7229 -- it had zero production callers after #7227; \
         restore a real caller instead of resurrecting unused RBAC surface."
    );
    assert!(
        !source.contains("Action::"),
        "auth.rs must not reference the removed `Action` enum (#7229)."
    );
}
