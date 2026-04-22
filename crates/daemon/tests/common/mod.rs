//! Shared helpers for the split `public_api_*.rs` integration test binaries.
//!
//! Cargo treats `tests/common/mod.rs` as a shared module: it is not compiled
//! as its own test binary. Each `tests/public_api_*.rs` re-declares
//! `mod common;` to pull in these helpers.
//!
//! WHY: extracted from the monolithic `tests/public_api.rs` (1316 lines) to
//! satisfy `RUST/file-too-long`.

#![expect(
    clippy::expect_used,
    reason = "test helpers — panicking on failure is the point"
)]
#![expect(
    dead_code,
    reason = "shared helpers: not every split file uses every helper"
)]

use std::path::Path;

use tokio_util::sync::CancellationToken;

use oikonomos::runner::TaskRunner;

/// Write a fixture file synchronously via `OpenOptions` + `Write`.
///
/// WHY: the daemon crate's `clippy.toml` disallows `std::fs::write` to steer
/// production code toward `tokio::fs`. Integration tests still inherit that
/// clippy config. Using explicit `File::create` + `write_all` is equivalent
/// and keeps the lint clean.
pub fn write_fixture(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path.as_ref())
        .expect("open fixture file");
    file.write_all(bytes.as_ref()).expect("write fixture bytes");
    file.flush().expect("flush fixture file");
}

/// Build a minimal `TaskRunner` bound to a throw-away cancellation token.
pub fn make_runner(nous_id: &str) -> TaskRunner {
    TaskRunner::new(nous_id, CancellationToken::new())
}
