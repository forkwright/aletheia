//! In-memory API response cache with TTL and request deduplication.
//!
//! # Design
//!
//! - `ApiCache` is `Send + Sync` and intended to be shared via `Arc<Mutex<ApiCache>>`.
//! - Entries expire after their TTL; [`ApiCache::get`] returns `None` for stale entries.
//! - Request deduplication prevents concurrent identical calls within a 500 ms window.
//!   Call [`ApiCache::mark_in_flight`] before issuing a request and
//!   [`ApiCache::mark_complete`] when it finishes.
//! - [`ApiCache::evict_expired`] prunes stale entries; call periodically or before insertion.

#[cfg(test)]
mod tests {
}
