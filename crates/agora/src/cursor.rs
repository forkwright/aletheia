//! Per-account provider resumption cursors.
//!
//! Matrix `/sync` returns a `next_batch` token that the next request must
//! replay as `since`; persisting it means a restart resumes after the last
//! accepted batch instead of replaying it (#7104). Agora is a
//! message-routing layer and owns no persistence (see this crate's
//! `clippy.toml`), so this module defines only the seam: the runtime injects
//! a store implementation via
//! [`crate::matrix::MatrixProvider::set_cursor_store`].
//!
//! Signal deliberately has no cursor: signal-cli's `receive` consumes
//! messages destructively on the daemon side, so a local cursor would be a
//! second, disagreeing source of truth (see [`crate::semeion`]).

use std::io;

/// Where channel providers keep their resumption cursors.
///
/// Implementations must not place `account` in any on-disk path: account
/// identifiers are phone numbers and Matrix user IDs, and paths leak into
/// directory listings, backups, and any error message that prints a path.
pub trait CursorStore: Send + Sync {
    /// Last persisted cursor for this channel+account, if any.
    ///
    /// # Errors
    ///
    /// Returns an error when a present cursor cannot be read or validated;
    /// absence alone is `Ok(None)`. Corruption must surface as an error, not
    /// as absence, because starting without a cursor replays batches this
    /// instance already accepted.
    fn load(&self, channel: &str, account: &str) -> io::Result<Option<String>>;

    /// Persist the cursor before the provider forwards the corresponding batch.
    ///
    /// # Errors
    ///
    /// Returns an error when the cursor cannot be made durable.
    fn save(&self, channel: &str, account: &str, cursor: &str) -> io::Result<()>;
}
