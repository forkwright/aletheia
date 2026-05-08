//! Doom-loop, ping-pong, and no-progress detection via canonical hash matching.
//!
//! # Integration seam
//!
//! This module provides three orthogonal detectors behind a composite
//! [`LoopGuard`]:
//!
//! * [`DoomLoopDetector`] — `k` consecutive identical tool-call signatures.
//! * [`PingPongDetector`] — strict A-B-A-B-A alternation between two distinct
//!   signatures.
//! * [`NoProgressDetector`] — `n` consecutive turns where the assistant produces
//!   the same canonical output despite firing tools.
//!
//! Wire `LoopGuard::record` after each turn's tool execution, and call
//! `LoopGuard::reset_on_user_message` on operator intervention or new user
//! message.
//!
//! # Hash function
//!
//! Uses `std::hash::DefaultHasher` (`SipHash` 1-3). Speed is sufficient for
//! ephemeral signatures, and it avoids adding a new dependency to the crate.

use std::collections::VecDeque;
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

    /// A-B-A-B-A oscillation between two distinct tool calls was observed.
    #[snafu(display(
        "ping-pong detected: alternating between {tool_a} and {tool_b} \
         ({k} signatures)"
    ))]
    PingPongDetected {
        /// Display identifier of the first tool.
        tool_a: String,
        /// Display identifier of the second tool.
        tool_b: String,
        /// Window size that triggered.
        k: usize,
    },

    /// `limit` consecutive turns produced the same assistant output without
    /// advancing state.
    #[snafu(display(
        "no progress detected: {consecutive} consecutive turns with identical \
         assistant output (limit {limit})"
    ))]
    NoProgressDetected {
        /// Current consecutive count.
        consecutive: u32,
        /// Threshold that triggered.
        limit: u32,
    },
}

/// Ring-buffer detector for repeated identical tool-call signatures.
///
/// Maintains the most recent `capacity` signatures. After each `record`,
/// checks whether the last `k` entries are identical; if so, returns
/// [`LoopDetectorError::DoomLoopDetected`].
#[derive(Debug, Clone)]
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

/// Detects A-B-A-B-A oscillation between two distinct tool calls.
///
/// Triggers when the last `k` signatures form a strict alternation pattern
/// between two distinct signatures.
#[derive(Debug, Clone)]
pub struct PingPongDetector {
    ring: VecDeque<ToolCallSignature>,
    capacity: usize,
    k: usize,
}

impl PingPongDetector {
    /// Create a new detector.
    ///
    /// # Panics
    ///
    /// Panics if `capacity == 0` or `k < 3`.
    #[must_use]
    pub fn new(capacity: usize, k: usize) -> Self {
        assert!(capacity > 0, "capacity must be > 0");
        assert!(k >= 3, "k must be >= 3 for meaningful ping-pong detection");
        Self {
            ring: VecDeque::with_capacity(capacity),
            capacity,
            k,
        }
    }

    /// Record a signature and check for a ping-pong pattern.
    ///
    /// If the last `k` entries form a strict A-B-A-B… alternation between
    /// two *distinct* signatures, returns `Err`. Otherwise the signature is
    /// stored and `Ok(())` is returned.
    ///
    /// # Errors
    ///
    /// Returns [`LoopDetectorError::PingPongDetected`] when the threshold is
    /// crossed.
    pub fn record(&mut self, sig: ToolCallSignature) -> Result<(), LoopDetectorError> {
        if self.ring.len() == self.capacity {
            self.ring.pop_front();
        }
        self.ring.push_back(sig);

        if self.ring.len() >= self.k {
            let tail: Vec<_> = self
                .ring
                .iter()
                .skip(self.ring.len() - self.k)
                .copied()
                .collect();
            if let Some((a, b)) = Self::is_strict_alternation(&tail) {
                return PingPongDetectedSnafu {
                    tool_a: format!("{:016x}", a.name_hash),
                    tool_b: format!("{:016x}", b.name_hash),
                    k: self.k,
                }
                .fail();
            }
        }

        Ok(())
    }

    /// Clear all recorded history.
    pub fn reset(&mut self) {
        self.ring.clear();
    }

    /// Check whether `slice` is a strict alternation between two distinct
    /// signatures.
    fn is_strict_alternation(
        slice: &[ToolCallSignature],
    ) -> Option<(ToolCallSignature, ToolCallSignature)> {
        if slice.len() < 3 {
            return None;
        }
        let a = slice.first().copied()?;
        let b = slice.get(1).copied()?;
        if a == b {
            return None;
        }
        for (i, sig) in slice.iter().enumerate() {
            let expected = if i % 2 == 0 { a } else { b };
            if *sig != expected {
                return None;
            }
        }
        Some((a, b))
    }
}

/// Detects `limit` consecutive turns that fire tool calls but don't advance
/// assistant state.
///
/// Compares the canonical hash of the assistant's turn output (content +
/// reasoning). If the hash repeats for `limit` consecutive turns where at
/// least one tool was called, the detector fires.
#[derive(Debug, Clone)]
pub struct NoProgressDetector {
    last_assistant_hash: Option<u64>,
    consecutive_no_progress: u32,
    limit: u32,
}

impl NoProgressDetector {
    /// Create a new detector.
    ///
    /// # Panics
    ///
    /// Panics if `limit == 0`.
    #[must_use]
    pub fn new(limit: u32) -> Self {
        assert!(limit > 0, "limit must be > 0");
        Self {
            last_assistant_hash: None,
            consecutive_no_progress: 0,
            limit,
        }
    }

    /// Record a turn and check for no-progress.
    ///
    /// * If `tool_called` is `true` and `assistant_hash` equals the previous
    ///   hash, the internal counter is incremented.
    /// * Otherwise the counter is reset to `0` (or `1` if this is the first
    ///   recorded turn with tools).
    /// * If the counter reaches `limit`, returns `Err`.
    ///
    /// A turn without tool calls does **not** increment the counter, but it
    /// also does **not** reset it. This prevents a single no-tool turn from
    /// breaking a legitimate no-progress streak that spans tool-use turns.
    ///
    /// # Errors
    ///
    /// Returns [`LoopDetectorError::NoProgressDetected`] when the threshold is
    /// crossed.
    pub fn record_turn(
        &mut self,
        assistant_hash: u64,
        tool_called: bool,
    ) -> Result<(), LoopDetectorError> {
        if tool_called {
            if self.last_assistant_hash == Some(assistant_hash) {
                self.consecutive_no_progress += 1;
            } else {
                self.consecutive_no_progress = 1;
            }
            self.last_assistant_hash = Some(assistant_hash);

            if self.consecutive_no_progress >= self.limit {
                return NoProgressDetectedSnafu {
                    consecutive: self.consecutive_no_progress,
                    limit: self.limit,
                }
                .fail();
            }
        }
        // No-tool turn: leave counter and last_hash unchanged.

        Ok(())
    }

    /// Clear all recorded history.
    pub fn reset(&mut self) {
        self.last_assistant_hash = None;
        self.consecutive_no_progress = 0;
    }
}

/// Composite guard that runs all three detectors behind a single entry point.
///
/// The actor calls [`LoopGuard::record`] once per turn; the guard fans out to
/// the underlying detectors and returns the first error encountered.
#[derive(Debug, Clone)]
pub struct LoopGuard {
    doom: DoomLoopDetector,
    ping_pong: PingPongDetector,
    no_progress: NoProgressDetector,
}

impl LoopGuard {
    /// Create a new guard with sensible defaults.
    ///
    /// * Doom-loop: capacity 20, threshold 3.
    /// * Ping-pong: capacity 20, threshold 5 (A-B-A-B-A).
    /// * No-progress: limit 3.
    #[must_use]
    pub fn new() -> Self {
        Self {
            doom: DoomLoopDetector::new(20, 3),
            ping_pong: PingPongDetector::new(20, 5),
            no_progress: NoProgressDetector::new(3),
        }
    }

    /// Create a new guard with custom thresholds.
    #[must_use]
    pub fn with_limits(doom_k: usize, ping_pong_k: usize, no_progress_limit: u32) -> Self {
        Self {
            doom: DoomLoopDetector::new(20, doom_k),
            ping_pong: PingPongDetector::new(20, ping_pong_k),
            no_progress: NoProgressDetector::new(no_progress_limit),
        }
    }

    /// Record a completed turn.
    ///
    /// 1. Computes the assistant hash from `content` and `reasoning`.
    /// 2. For each tool call (name, args, result), builds a
    ///    [`ToolCallSignature`] and records it in the doom-loop and ping-pong
    ///    detectors. The first error short-circuits.
    /// 3. Records the turn in the no-progress detector.
    ///
    /// # Errors
    ///
    /// Returns the first [`LoopDetectorError`] encountered.
    pub fn record(
        &mut self,
        content: &str,
        reasoning: &str,
        tool_calls: &[(&str, &str, &str)],
    ) -> Result<(), LoopDetectorError> {
        // 1. Compute canonical assistant hash.
        let assistant_hash = Self::assistant_hash(content, reasoning);
        let tool_called = !tool_calls.is_empty();

        // 2. Record each tool call in doom + ping-pong.
        for (name, args, result) in tool_calls {
            let sig = ToolCallSignature::from_parts(name, args, result);
            // Doom is checked first: it is the more specific detector (exact
            // repetition) and should win over ping-pong when both could fire.
            self.doom.record(sig)?;
            self.ping_pong.record(sig)?;
        }

        // 3. Check no-progress across turns.
        self.no_progress.record_turn(assistant_hash, tool_called)?;

        Ok(())
    }

    /// Reset all three detectors. Call this when a user message arrives or
    /// an operator intervenes to break a potential loop.
    pub fn reset_on_user_message(&mut self) {
        self.doom.reset();
        self.ping_pong.reset();
        self.no_progress.reset();
    }

    /// Canonical hash of assistant output for no-progress detection.
    ///
    /// Combines `content` and `reasoning` so that a turn with identical text
    /// but different reasoning counts as a change.
    fn assistant_hash(content: &str, reasoning: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        reasoning.hash(&mut hasher);
        hasher.finish()
    }
}

impl Default for LoopGuard {
    fn default() -> Self {
        Self::new()
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
mod tests;
