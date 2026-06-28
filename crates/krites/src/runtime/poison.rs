//! Query cancellation token and budget accounting.
//!
//! Defines [`Poison`] for cooperative query cancellation and [`QueryBudget`]
//! for engine-level resource limits. Also exports [`QueryCancellationReason`]
//! and [`DEFAULT_MAX_EVALUATION_EPOCHS`] used across the runtime.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use snafu::Snafu;

use crate::error::InternalResult as Result;
use crate::runtime::error::{QueryCancelledSnafu, UnsupportedSnafu};

/// Default maximum semi-naive evaluation epochs for one stratum.
pub const DEFAULT_MAX_EVALUATION_EPOCHS: u32 = 10_000;

pub(crate) const CANCELLATION_NONE: u8 = 0;
const CANCELLATION_EXPLICIT: u8 = 1;
const CANCELLATION_TIMEOUT: u8 = 2;
const CANCELLATION_DERIVED_ROWS: u8 = 3;
const CANCELLATION_WORK_UNITS: u8 = 4;
const CANCELLATION_EPOCH_LIMIT: u8 = 5;

/// Structured reason a query budget stopped evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum QueryCancellationReason {
    /// The operator explicitly killed the running query.
    ExplicitKill,
    /// The query exceeded its wall-clock timeout.
    Timeout,
    /// The query exhausted its configured semi-naive epoch budget.
    EpochLimit,
    /// The query derived more rows than its configured budget permits.
    DerivedRowLimit,
    /// The query consumed more evaluator work units than its configured budget permits.
    WorkUnitLimit,
}

impl QueryCancellationReason {
    pub(crate) fn as_u8(self) -> u8 {
        match self {
            QueryCancellationReason::ExplicitKill => CANCELLATION_EXPLICIT,
            QueryCancellationReason::Timeout => CANCELLATION_TIMEOUT,
            QueryCancellationReason::EpochLimit => CANCELLATION_EPOCH_LIMIT,
            QueryCancellationReason::DerivedRowLimit => CANCELLATION_DERIVED_ROWS,
            QueryCancellationReason::WorkUnitLimit => CANCELLATION_WORK_UNITS,
        }
    }

    pub(crate) fn from_u8(raw: u8) -> Option<Self> {
        match raw {
            CANCELLATION_EXPLICIT => Some(Self::ExplicitKill),
            CANCELLATION_TIMEOUT => Some(Self::Timeout),
            CANCELLATION_EPOCH_LIMIT => Some(Self::EpochLimit),
            CANCELLATION_DERIVED_ROWS => Some(Self::DerivedRowLimit),
            CANCELLATION_WORK_UNITS => Some(Self::WorkUnitLimit),
            _ => None,
        }
    }
}

impl std::fmt::Display for QueryCancellationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryCancellationReason::ExplicitKill => write!(f, "explicit_kill"),
            QueryCancellationReason::Timeout => write!(f, "timeout"),
            QueryCancellationReason::EpochLimit => write!(f, "epoch_limit"),
            QueryCancellationReason::DerivedRowLimit => write!(f, "derived_row_limit"),
            QueryCancellationReason::WorkUnitLimit => write!(f, "work_unit_limit"),
        }
    }
}

/// Engine-level resource budget for one query execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct QueryBudget {
    /// Optional wall-clock timeout measured from query start.
    pub wall_clock_timeout: Option<Duration>,
    /// Maximum semi-naive evaluation epochs per stratum.
    pub max_epochs: u32,
    /// Optional maximum number of newly derived rows/facts.
    pub max_derived_rows: Option<u64>,
    /// Optional maximum evaluator work-unit count.
    pub max_work_units: Option<u64>,
}

impl Default for QueryBudget {
    fn default() -> Self {
        Self {
            wall_clock_timeout: None,
            max_epochs: DEFAULT_MAX_EVALUATION_EPOCHS,
            max_derived_rows: None,
            max_work_units: None,
        }
    }
}

impl QueryBudget {
    /// Return an unrestricted budget except for the default epoch guard.
    #[must_use]
    pub fn default_query() -> Self {
        Self::default()
    }

    /// Return a budget with no wall-clock, row, or work-unit limit.
    #[must_use]
    pub fn unbounded() -> Self {
        Self {
            wall_clock_timeout: None,
            max_epochs: u32::MAX,
            max_derived_rows: None,
            max_work_units: None,
        }
    }

    /// Add a wall-clock timeout.
    #[must_use]
    pub fn with_wall_clock_timeout(mut self, timeout: Duration) -> Self {
        self.wall_clock_timeout = Some(timeout);
        self
    }

    /// Set the semi-naive epoch cap.
    #[must_use]
    pub fn with_max_epochs(mut self, max_epochs: u32) -> Self {
        self.max_epochs = max_epochs;
        self
    }

    /// Set the derived-row cap.
    #[must_use]
    pub fn with_max_derived_rows(mut self, max_derived_rows: u64) -> Self {
        self.max_derived_rows = Some(max_derived_rows);
        self
    }

    /// Set the work-unit cap.
    #[must_use]
    pub fn with_max_work_units(mut self, max_work_units: u64) -> Self {
        self.max_work_units = Some(max_work_units);
        self
    }
}

pub(crate) fn saturating_secs_f64(secs: f64) -> Duration {
    if secs.is_nan() || secs <= 0.0 {
        return Duration::ZERO;
    }
    if secs.is_infinite() || secs >= Duration::MAX.as_secs_f64() {
        return Duration::MAX;
    }
    Duration::from_secs_f64(secs)
}

fn millis_to_u64(millis: u128) -> u64 {
    u64::try_from(millis).unwrap_or(u64::MAX)
}

#[derive(Debug)]
pub(crate) struct QueryBudgetState {
    budget: QueryBudget,
    started_at: Instant,
    // WHY: timeout can come from parsed query options after token construction.
    wall_clock_timeout: parking_lot::Mutex<Option<Duration>>,
    cancelled: AtomicBool,
    reason: AtomicU8,
    work_units: AtomicU64,
    derived_rows: AtomicU64,
}

impl QueryBudgetState {
    pub(crate) fn new(budget: QueryBudget) -> Self {
        Self {
            budget,
            started_at: Instant::now(),
            wall_clock_timeout: parking_lot::Mutex::new(budget.wall_clock_timeout),
            cancelled: AtomicBool::default(),
            reason: AtomicU8::new(CANCELLATION_NONE),
            work_units: AtomicU64::default(),
            derived_rows: AtomicU64::default(),
        }
    }

    fn mark_cancelled(&self, reason: QueryCancellationReason) {
        let _ = self.reason.compare_exchange(
            CANCELLATION_NONE,
            reason.as_u8(),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.cancelled.store(true, Ordering::Release);
    }

    fn reason(&self) -> QueryCancellationReason {
        QueryCancellationReason::from_u8(self.reason.load(Ordering::Acquire))
            .unwrap_or(QueryCancellationReason::ExplicitKill)
    }
}

/// Typed error for query cancellation: enables downstream matching without string parsing.
#[derive(Debug, Snafu)]
#[snafu(display("Running query is killed before completion"))]
pub(crate) struct ProcessKilled;

/// Used for user-initiated termination and query-budget accounting.
#[derive(Clone)]
pub struct Poison {
    state: Arc<QueryBudgetState>,
}

impl Default for Poison {
    fn default() -> Self {
        Self::new(QueryBudget::unbounded())
    }
}

impl Poison {
    /// Create a cancellation token backed by an engine query budget.
    #[must_use]
    pub fn new(budget: QueryBudget) -> Self {
        Self {
            state: Arc::new(QueryBudgetState::new(budget)),
        }
    }

    /// Check whether the query has been cancelled or has passed its timeout.
    ///
    /// # Errors
    ///
    /// Returns a query-killed error if the user initiated termination or the
    /// timeout deadline has elapsed.
    #[must_use = "caller must propagate the query-killed error"]
    #[inline(always)]
    pub fn check(&self) -> Result<()> {
        if self.state.cancelled.load(Ordering::Acquire) {
            Self::fail_cancelled(self.state.reason(), None)?;
        }
        if let Some(timeout) = *self.state.wall_clock_timeout.lock()
            && self.state.started_at.elapsed() >= timeout
        {
            self.state.mark_cancelled(QueryCancellationReason::Timeout);
            Self::fail_cancelled(
                QueryCancellationReason::Timeout,
                Some((
                    millis_to_u64(self.state.started_at.elapsed().as_millis()),
                    millis_to_u64(timeout.as_millis()),
                )),
            )?;
        }
        Ok(())
    }

    /// Mark the query as killed without waiting for a timeout.
    pub(crate) fn set_killed(&self) {
        self.state
            .mark_cancelled(QueryCancellationReason::ExplicitKill);
    }

    pub(crate) fn max_epochs(&self) -> u32 {
        self.state.budget.max_epochs
    }

    pub(crate) fn account_work(&self, units: u64) -> Result<()> {
        self.check()?;
        let Some(limit) = self.state.budget.max_work_units else {
            return Ok(());
        };
        let observed = self
            .state
            .work_units
            .fetch_add(units, Ordering::AcqRel)
            .saturating_add(units);
        if observed > limit {
            self.state
                .mark_cancelled(QueryCancellationReason::WorkUnitLimit);
            Self::fail_cancelled(
                QueryCancellationReason::WorkUnitLimit,
                Some((observed, limit)),
            )?;
        }
        Ok(())
    }

    pub(crate) fn account_derived_rows(&self, rows: u64) -> Result<()> {
        self.check()?;
        if rows == 0 {
            return Ok(());
        }
        let Some(limit) = self.state.budget.max_derived_rows else {
            return Ok(());
        };
        let observed = self
            .state
            .derived_rows
            .fetch_add(rows, Ordering::AcqRel)
            .saturating_add(rows);
        if observed > limit {
            self.state
                .mark_cancelled(QueryCancellationReason::DerivedRowLimit);
            Self::fail_cancelled(
                QueryCancellationReason::DerivedRowLimit,
                Some((observed, limit)),
            )?;
        }
        Ok(())
    }

    pub(crate) fn mark_epoch_limit(&self) {
        self.state
            .mark_cancelled(QueryCancellationReason::EpochLimit);
    }

    fn fail_cancelled(
        reason: QueryCancellationReason,
        observed_limit: Option<(u64, u64)>,
    ) -> Result<()> {
        let (observed, limit) = observed_limit.map_or((None, None), |(observed, limit)| {
            (Some(observed), Some(limit))
        });
        QueryCancelledSnafu {
            reason,
            observed,
            limit,
        }
        .fail()?
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn set_timeout(&self, _secs: f64) -> Result<()> {
        UnsupportedSnafu {
            operation: "set timeout",
            reason: "threading is disallowed on this platform",
        }
        .fail()?
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn set_timeout(&self, secs: f64) -> Result<()> {
        *self.state.wall_clock_timeout.lock() = Some(saturating_secs_f64(secs));
        Ok(())
    }
}
