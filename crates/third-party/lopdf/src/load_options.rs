use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::{DecompressError, Error, Object, Result};

/// Shared, fail-closed decompression budget for one untrusted PDF operation.
///
/// Reservations deliberately charge the caller's maximum per-decoder bound,
/// rather than attempting to refund a partial decode. This makes concurrent
/// object-stream loading safe: no collection of individually valid streams can
/// exceed the operation-wide allowance.
#[derive(Clone, Debug)]
pub struct DecompressionBudget(Arc<Mutex<BudgetState>>);

#[derive(Debug)]
struct BudgetState {
    limit: usize,
    remaining: usize,
}

impl DecompressionBudget {
    /// Create a budget that can be shared by loading and later extraction.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self(Arc::new(Mutex::new(BudgetState {
            limit,
            remaining: limit,
        })))
    }

    /// Reserve the maximum output of one bounded decoder.
    ///
    /// # Errors
    ///
    /// Returns [`DecompressError::MemoryLimitExceeded`] before decoding when
    /// the operation-wide allowance cannot cover this decoder.
    pub fn reserve(&self, bytes: usize) -> Result<()> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| DecompressError::MemoryLimitExceeded { limit: 0 })?;
        if bytes > state.remaining {
            return Err(DecompressError::MemoryLimitExceeded { limit: state.limit }.into());
        }
        state.remaining -= bytes;
        Ok(())
    }

    /// Bytes still available for bounded decoding.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.0.lock().map_or(0, |state| state.remaining)
    }
}

/// Shared, fail-closed admission budget for bytes copied from a PDF into
/// document-owned storage during one load operation.
///
/// This is distinct from [`DecompressionBudget`]: direct, unfiltered streams
/// still require `Vec` copies while the document loads. Every allocation is
/// charged: object IDs identify PDF syntax, not ownership of one `Vec`, and a
/// reentrant or parallel lookup may create a second live copy of the same
/// object's source bytes.
#[derive(Clone, Debug)]
pub struct RetainedBytesBudget(Arc<Mutex<RetainedBytesState>>);

#[derive(Debug)]
struct RetainedBytesState {
    limit: usize,
    remaining: usize,
}

impl RetainedBytesBudget {
    /// Create a budget shared by all source-byte copies made while loading one
    /// document.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self(Arc::new(Mutex::new(RetainedBytesState {
            limit,
            remaining: limit,
        })))
    }

    /// Reserve one retained allocation before its `Vec` copy occurs.
    ///
    /// This is deliberately cumulative rather than deduplicated: each call
    /// corresponds to one new owned allocation, including reparses of the
    /// same indirect object.
    pub(crate) fn reserve(&self, bytes: usize) -> Result<()> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| Error::RetainedBytesLimitExceeded { limit: 0 })?;
        if bytes > state.remaining {
            return Err(Error::RetainedBytesLimitExceeded { limit: state.limit });
        }
        state.remaining -= bytes;
        Ok(())
    }

    /// Bytes still available for source-byte copies retained by the loader.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.0.lock().map_or(0, |state| state.remaining)
    }
}

/// Shared budget for distinct PDF source intervals inspected by one load.
///
/// This limits parser work without pretending that borrowed source bytes are
/// retained allocations. Re-reading an already admitted interval is free;
/// overlapping intervals charge only their newly covered bytes.
#[derive(Clone, Debug)]
pub struct SourceWorkBudget(Arc<Mutex<SourceWorkState>>);

#[derive(Debug)]
struct SourceWorkState {
    limit: usize,
    charged: usize,
    intervals: BTreeMap<usize, usize>,
}

impl SourceWorkBudget {
    /// Create a source-work budget shared by all parser paths in one load.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self(Arc::new(Mutex::new(SourceWorkState {
            limit,
            charged: 0,
            intervals: BTreeMap::new(),
        })))
    }

    /// Admit an input interval before parsing it.
    pub(crate) fn reserve_interval(&self, start: usize, end: usize) -> Result<()> {
        if start > end {
            return Err(Error::InvalidOffset(start));
        }
        let mut state = self.0.lock().map_err(|_| Error::SourceWorkLimitExceeded { limit: 0 })?;
        let mut merged_start = start;
        let mut merged_end = end;
        let mut replaced = 0usize;

        if let Some((&previous_start, &previous_end)) = state.intervals.range(..=start).next_back()
            && previous_end >= start
        {
            merged_start = previous_start;
            merged_end = merged_end.max(previous_end);
            replaced = replaced.saturating_add(previous_end.saturating_sub(previous_start));
        }
        let overlaps: Vec<(usize, usize)> = state
            .intervals
            .range(start..)
            .take_while(|(interval_start, _)| **interval_start <= merged_end)
            .map(|(&interval_start, &interval_end)| (interval_start, interval_end))
            .collect();
        for (interval_start, interval_end) in &overlaps {
            merged_start = merged_start.min(*interval_start);
            merged_end = merged_end.max(*interval_end);
            if *interval_start != merged_start {
                replaced = replaced.saturating_add(interval_end.saturating_sub(*interval_start));
            }
        }
        let merged_len = merged_end.saturating_sub(merged_start);
        let next_charged = state.charged.saturating_sub(replaced).saturating_add(merged_len);
        if next_charged > state.limit {
            return Err(Error::SourceWorkLimitExceeded { limit: state.limit });
        }
        for (interval_start, _) in overlaps {
            state.intervals.remove(&interval_start);
        }
        state.intervals.insert(merged_start, merged_end);
        state.charged = next_charged;
        Ok(())
    }
}

/// Type alias for the filter function used during PDF loading.
///
/// The function receives an object ID and a mutable reference to the object,
/// and returns `Some((id, object))` to keep it or `None` to discard it.
pub type FilterFunc = fn((u32, u16), &mut Object) -> Option<((u32, u16), Object)>;

/// Options for loading PDF documents.
///
/// Use this struct to configure password, object filtering, and strictness
/// when loading a PDF. The default is lenient parsing with no password or filter.
///
/// # Examples
///
/// ```no_run
/// use lopdf::{Document, LoadOptions};
///
/// // Load with a password
/// let doc = Document::load_with_options(
///     "encrypted.pdf",
///     LoadOptions::with_password("secret"),
/// );
///
/// // Load with strict parsing
/// let doc = Document::load_with_options(
///     "document.pdf",
///     LoadOptions { strict: true, ..Default::default() },
/// );
/// ```
#[derive(Clone, Default)]
pub struct LoadOptions {
    /// Password for encrypted PDFs.
    pub password: Option<String>,
    /// Object filter applied during loading.
    pub filter: Option<FilterFunc>,
    /// When `true`, reject non-conforming PDFs instead of silently accepting them.
    /// Defaults to `false` (lenient parsing).
    pub strict: bool,
    /// Maximum number of bytes any single stream may decompress to during
    /// loading (object streams and cross-reference streams).
    ///
    /// Compression filters can inflate a tiny input into an enormous output (a
    /// "decompression bomb"). Because object and xref streams are decoded eagerly
    /// while the document is loaded, an unbounded stream can exhaust memory
    /// before any of your code runs. Set this to bound that per-stream cost when
    /// loading untrusted PDFs; a stream that would exceed it fails with
    /// [`crate::DecompressError::MemoryLimitExceeded`].
    ///
    /// `None` (the default) applies no limit.
    pub max_decompressed_size: Option<usize>,
    /// Shared aggregate decompression budget for xref/object streams loaded
    /// eagerly and for caller-selected bounded extraction APIs.
    ///
    /// When supplied, every eager xref/object-stream decoder reserves its
    /// configured per-stream maximum before it runs. Retain a clone and pass it
    /// to [`crate::Document::extract_text_chunks_with_limit_and_budget`] so the
    /// load and extraction stages share one document-wide budget. That method
    /// also accepts a [`crate::ToUnicodeMappingBudget`] for aggregate CMap
    /// admission.
    pub decompression_budget: Option<DecompressionBudget>,
    /// Shared aggregate budget for source bytes copied into loader-owned
    /// stream buffers, object-stream members, encrypted staging, and explicit
    /// object clones during loading.
    ///
    /// Unlike [`LoadOptions::decompression_budget`], this covers direct
    /// `/Length` streams reserve immediately before their raw bytes are copied.
    /// Reparse/clone generations charge cumulatively. Structural parsing of
    /// borrowed names, strings, dictionaries, and xref spans is bounded by
    /// [`LoadOptions::source_work_budget`] instead.
    pub retained_bytes_budget: Option<RetainedBytesBudget>,
    /// Shared aggregate budget for distinct borrowed source intervals the
    /// loader parses. This bounds structural/parser work separately from
    /// [`LoadOptions::retained_bytes_budget`].
    pub source_work_budget: Option<SourceWorkBudget>,
    /// Maximum unique indirect object IDs admitted across the merged
    /// cross-reference graph and every object-stream member.
    ///
    /// Object-stream members must be declared by that bounded xref graph, so
    /// they cannot amplify the retained object set after admission. `None`
    /// preserves lopdf's historical unbounded API; security-sensitive callers
    /// must set this before parsing attacker-controlled input.
    pub max_objects: Option<usize>,
    /// Reject a trailer with `/Encrypt` before password authentication or
    /// decryption. This is for callers that do not support encrypted input.
    pub reject_encrypted: bool,
}

impl std::fmt::Debug for LoadOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadOptions")
            .field("password", &self.password.as_ref().map(|_| "***"))
            .field("filter", &self.filter.map(|_| "fn(..)"))
            .field("strict", &self.strict)
            .field("max_decompressed_size", &self.max_decompressed_size)
            .field("decompression_budget", &self.decompression_budget)
            .field("retained_bytes_budget", &self.retained_bytes_budget)
            .field("source_work_budget", &self.source_work_budget)
            .field("max_objects", &self.max_objects)
            .field("reject_encrypted", &self.reject_encrypted)
            .finish()
    }
}

impl LoadOptions {
    /// Create options with a password for encrypted PDFs.
    pub fn with_password(password: &str) -> Self {
        Self {
            password: Some(password.to_string()),
            ..Default::default()
        }
    }

    /// Create options with an object filter.
    pub fn with_filter(filter: FilterFunc) -> Self {
        Self {
            filter: Some(filter),
            ..Default::default()
        }
    }

    /// Create options that bound how large any single stream may decompress to
    /// during loading, to defend against decompression bombs in untrusted PDFs.
    /// See [`LoadOptions::max_decompressed_size`].
    pub fn with_max_decompressed_size(max_decompressed_size: usize) -> Self {
        Self {
            max_decompressed_size: Some(max_decompressed_size),
            ..Default::default()
        }
    }
}
