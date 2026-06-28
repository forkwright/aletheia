//! Query-budget and cancellation tests.
#![expect(clippy::expect_used, reason = "test assertions")]

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use compact_str::CompactString;

use crate::DbInstance;
use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::{InternalError, InternalResult as Result};
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::query::error::QueryError;
use crate::runtime::db::{DbConfig, Poison, QueryCancellationReason};
use crate::runtime::error::RuntimeError;
use crate::runtime::temp_store::RegularTempStore;

fn assert_cancelled(err: InternalError, expected: QueryCancellationReason) {
    match err {
        InternalError::Runtime {
            source: RuntimeError::QueryCancelled { reason, .. },
        } => assert_eq!(reason, expected),
        other => panic!("expected query cancellation {expected:?}, got {other:?}"),
    }
}

#[test]
fn wall_clock_timeout_reports_structured_reason() {
    let db = DbInstance::default();
    db.register_fixed_rule("SlowBudget".to_string(), SlowBudget)
        .expect("registering SlowBudget rule should succeed");

    let err = db
        .run_default(
            r"
            seed[] <- [[1]]
            ?[x] <~ SlowBudget(seed[]) :timeout 0.001
            ",
        )
        .expect_err("slow fixed rule should exceed wall-clock timeout");

    assert_cancelled(err, QueryCancellationReason::Timeout);
}

#[test]
fn epoch_limit_reports_structured_epoch_error() {
    let mut db = DbInstance::default();
    db.config = DbConfig::new(2);

    let err = db
        .run_default(
            r"
            looped[id] := id = rand_uuid_v4()
            looped[id] := looped[prev], id = rand_uuid_v4(), prev != id
            ?[id] := looped[id]
            ",
        )
        .expect_err("non-converging recursive query should hit epoch limit");

    match err {
        InternalError::Query {
            source:
                QueryError::EpochLimitExceeded {
                    epoch_count,
                    max_epochs,
                    rule_context,
                    ..
                },
        } => {
            assert_eq!(epoch_count, 2);
            assert_eq!(max_epochs, 2);
            assert!(rule_context.contains("looped"));
        }
        other => panic!("expected epoch limit error, got {other:?}"),
    }
}

#[test]
fn derived_row_limit_cancels_query() {
    let mut db = DbInstance::default();
    db.config = DbConfig::new(100).with_max_derived_rows(2);

    let err = db
        .run_default("?[x] := x in [1, 2, 3]")
        .expect_err("query should exceed derived-row budget");

    assert_cancelled(err, QueryCancellationReason::DerivedRowLimit);
}

#[test]
fn fixed_rule_can_trip_work_unit_budget() {
    let mut db = DbInstance::default();
    db.config = DbConfig::new(100).with_max_work_units(2);
    db.register_fixed_rule("WorkBudget".to_string(), WorkBudget)
        .expect("registering WorkBudget rule should succeed");

    let err = db
        .run_default(
            r"
            seed[] <- [[1]]
            ?[x] <~ WorkBudget(seed[])
            ",
        )
        .expect_err("fixed rule should exceed work-unit budget");

    assert_cancelled(err, QueryCancellationReason::WorkUnitLimit);
}

struct SlowBudget;

impl FixedRule for SlowBudget {
    fn arity(
        &self,
        _options: &BTreeMap<CompactString, Expr>,
        _rule_head: &[Symbol],
        _span: SourceSpan,
    ) -> Result<usize> {
        Ok(1)
    }

    fn run(
        &self,
        _payload: FixedRulePayload<'_, '_>,
        out: &'_ mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(20) {
            poison.check()?;
        }
        out.put(vec![DataValue::from(1)]);
        Ok(())
    }
}

struct WorkBudget;

impl FixedRule for WorkBudget {
    fn arity(
        &self,
        _options: &BTreeMap<CompactString, Expr>,
        _rule_head: &[Symbol],
        _span: SourceSpan,
    ) -> Result<usize> {
        Ok(1)
    }

    fn run(
        &self,
        _payload: FixedRulePayload<'_, '_>,
        out: &'_ mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        poison.account_work(1)?;
        poison.account_work(1)?;
        out.put(vec![DataValue::from(1)]);
        Ok(())
    }
}
