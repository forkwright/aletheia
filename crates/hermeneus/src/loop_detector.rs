//! Doom-loop detection via `(args_hash, result_hash)` signature ring.
//!
//! # Integration seam
//!
//! This module provides the primitive (`DoomLoopDetector`) ready for the
//! Phase 7 multi-tool MCP dispatch path. Wire `DoomLoopDetector::record`
//! after each tool-call result lands, and call `DoomLoopDetector::reset` on
//! user message or operator intervention.
//!
//! # Hash function
//!
//! Uses `std::hash::DefaultHasher` (`SipHash` 1-3). Speed is sufficient for
//! ephemeral signatures, and it avoids adding a new dependency to the crate.

use std::hash::{DefaultHasher, Hash, Hasher};

use snafu::Snafu;

/// Signature of a single tool call for loop detection.
///
/// Two signatures are equal when all three hash fields match. The
/// `result_hash` field is load-bearing: naive args-only detection
/// false-triggers on legitimate polling (e.g. `tail` returning different
/// bytes each call).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToolCallSignature {
    /// `SipHash` of the tool name.
    pub name_hash: u64,
    /// `SipHash` of the serialized arguments.
    pub args_hash: u64,
    /// `SipHash` of the serialized result.
    pub result_hash: u64,
}

impl ToolCallSignature {
    /// Build a signature from raw name, args, and result bytes.
    ///
    /// Each component is hashed with `std::hash::DefaultHasher`.
    #[must_use]
    pub fn from_parts(name: &str, args: &str, result: &str) -> Self {
        Self {
            name_hash: hash_of(name),
            args_hash: hash_of(args),
            result_hash: hash_of(result),
        }
    }
}

/// Errors emitted by the loop detector.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum LoopDetectorError {
    /// `k` consecutive identical tool calls were observed.
    #[snafu(display("doom loop detected: {k} consecutive identical calls for tool {tool}"))]
    DoomLoopDetected {
        /// Display name of the tool that looped.
        tool: String,
        /// Threshold that triggered.
        k: usize,
    },
}

/// Ring-buffer detector for repeated identical tool-call signatures.
///
/// Maintains the most recent `capacity` signatures. After each `record`,
/// checks whether the last `k` entries are identical; if so, returns
/// [`LoopDetectorError::DoomLoopDetected`].
pub struct DoomLoopDetector {
    ring: Vec<ToolCallSignature>,
    capacity: usize,
    k: usize,
}

impl DoomLoopDetector {
    /// Create a new detector.
    ///
    /// # Panics
    ///
    /// Panics if `capacity == 0` or `k == 0`.
    #[must_use]
    pub fn new(capacity: usize, k: usize) -> Self {
        assert!(capacity > 0, "capacity must be > 0");
        assert!(k > 0, "k must be > 0");
        Self {
            ring: Vec::with_capacity(capacity),
            capacity,
            k,
        }
    }

    /// Record a signature and check for a doom loop.
    ///
    /// If `k` consecutive identical signatures are present in the ring,
    /// returns `Err`. Otherwise the signature is stored and `Ok(())` is
    /// returned.
    ///
    /// # Errors
    ///
    /// Returns [`LoopDetectorError::DoomLoopDetected`] when the threshold
    /// is crossed.
    pub fn record(&mut self, sig: ToolCallSignature) -> Result<(), LoopDetectorError> {
        self.ring.push(sig);
        if self.ring.len() > self.capacity {
            self.ring.remove(0);
        }

        if self.ring.len() >= self.k
            && let Some(tail) = self.ring.get(self.ring.len() - self.k..)
            && let Some(first) = tail.first()
            && tail.iter().all(|s| s == first)
        {
            return DoomLoopDetectedSnafu {
                tool: format!("{:016x}", first.name_hash),
                k: self.k,
            }
            .fail();
        }

        Ok(())
    }

    /// Clear all recorded history.
    pub fn reset(&mut self) {
        self.ring.clear();
    }
}

/// Hash a single value with `std::hash::DefaultHasher`.
#[must_use]
fn hash_of<T: Hash>(value: T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn sig(name: &str, args: &str, result: &str) -> ToolCallSignature {
        ToolCallSignature::from_parts(name, args, result)
    }

    #[test]
    fn polling_does_not_trigger() {
        let mut detector = DoomLoopDetector::new(10, 3);
        let base = sig("tail", "{\"file\":\"/var/log/syslog\"}", "");

        for i in 0..5 {
            let mut s = base;
            s.result_hash = hash_of(format!("line {i}"));
            detector.record(s).unwrap();
        }
    }

    #[test]
    fn identical_calls_trigger() {
        let mut detector = DoomLoopDetector::new(10, 3);
        let s = sig("cat", "{\"file\":\"/etc/hosts\"}", "127.0.0.1 localhost");

        detector.record(s).unwrap();
        detector.record(s).unwrap();
        let err = detector.record(s).unwrap_err();

        assert!(
            matches!(err, LoopDetectorError::DoomLoopDetected { k: 3, .. }),
            "expected DoomLoopDetected with k=3, got {err:?}"
        );
    }

    #[test]
    fn ring_capacity_evicts_oldest() {
        let mut detector = DoomLoopDetector::new(10, 3);
        let s = sig("echo", "{\"msg\":\"hi\"}", "hi");

        // First two calls are Ok; from the 3rd onward the detector fires.
        detector.record(s).unwrap();
        detector.record(s).unwrap();
        for _ in 2..11 {
            detector.record(s).unwrap_err();
        }

        // After 11 records with capacity 10, the ring contains exactly 10
        // entries (the first was evicted).
        assert_eq!(detector.ring.len(), 10);
    }

    #[test]
    fn reset_clears_history() {
        let mut detector = DoomLoopDetector::new(10, 3);
        let s = sig("echo", "{\"msg\":\"hi\"}", "hi");

        detector.record(s).unwrap();
        detector.record(s).unwrap();
        detector.record(s).unwrap_err(); // fires

        detector.reset();
        detector.record(s).unwrap(); // should not fire after reset
    }

    #[test]
    fn capacity_eviction_avoids_false_trigger() {
        // Fill ring with 10 unique signatures, then add an 11th that
        // duplicates the 1st (now evicted). Because the 1st is gone, the
        // duplicate should not form a run of 3.
        let mut detector = DoomLoopDetector::new(10, 3);
        let dup = sig("dup", "{}", "a");

        detector.record(dup).unwrap();
        for i in 0..9 {
            detector
                .record(sig("other", "{}", &format!("{i}")))
                .unwrap();
        }
        // Ring is now full; first `dup` is at index 0.
        detector.record(dup).unwrap(); // 11th record evicts first dup
        detector.record(dup).unwrap(); // 12th record — only 2 consecutive dups at tail
    }
}
