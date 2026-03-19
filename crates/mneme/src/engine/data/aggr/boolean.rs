//! Boolean and collection aggregation operators.
#![expect(
    clippy::as_conversions,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::collections::{BTreeMap, BTreeSet};

use rand::prelude::*;

use super::super::error::*;

type Result<T> = DataResult<T>;

use crate::engine::data::value::DataValue;

use super::{MeetAggrObj, NormalAggrObj};

pub(crate) struct AggrAnd {
    accum: bool,
}

impl Default for AggrAnd {
    fn default() -> Self {
        Self { accum: true }
    }
}

impl NormalAggrObj for AggrAnd {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::Bool(v) => self.accum &= *v,
            v => {
                return TypeMismatchSnafu {
                    op: "and",
                    expected: format!("compatible type, got {v:?}"),
                }
                .fail();
            }
        }
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        Ok(DataValue::from(self.accum))
    }
}

pub(crate) struct MeetAggrAnd;

impl MeetAggrObj for MeetAggrAnd {
    fn init_val(&self) -> DataValue {
        DataValue::from(true)
    }

    fn update(&self, left: &mut DataValue, right: &DataValue) -> Result<bool> {
        match (left, right) {
            (DataValue::Bool(l), DataValue::Bool(r)) => {
                let old = *l;
                *l &= *r;
                Ok(old == *l)
            }
            (u, v) => TypeMismatchSnafu {
                op: "and",
                expected: format!("compatible type, got {u:?} and {v:?}"),
            }
            .fail(),
        }
    }
}

#[derive(Default)]
pub(crate) struct AggrOr {
    accum: bool,
}

impl NormalAggrObj for AggrOr {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::Bool(v) => self.accum |= *v,
            v => {
                return TypeMismatchSnafu {
                    op: "or",
                    expected: format!("compatible type, got {v:?}"),
                }
                .fail();
            }
        }
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        Ok(DataValue::from(self.accum))
    }
}

pub(crate) struct MeetAggrOr;

impl MeetAggrObj for MeetAggrOr {
    fn init_val(&self) -> DataValue {
        DataValue::from(false)
    }

    fn update(&self, left: &mut DataValue, right: &DataValue) -> Result<bool> {
        match (left, right) {
            (DataValue::Bool(l), DataValue::Bool(r)) => {
                let old = *l;
                *l |= *r;
                Ok(old == *l)
            }
            (u, v) => TypeMismatchSnafu {
                op: "or",
                expected: format!("compatible type, got {u:?} and {v:?}"),
            }
            .fail(),
        }
    }
}

#[derive(Default)]
pub(crate) struct AggrUnique {
    accum: BTreeSet<DataValue>,
}

impl NormalAggrObj for AggrUnique {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        self.accum.insert(value.clone());
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        Ok(DataValue::List(self.accum.iter().cloned().collect()))
    }
}

#[derive(Default)]
pub(crate) struct AggrGroupCount {
    accum: BTreeMap<DataValue, i64>,
}

impl NormalAggrObj for AggrGroupCount {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        let entry = self.accum.entry(value.clone()).or_default();
        *entry += 1;
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        Ok(DataValue::List(
            self.accum
                .iter()
                .map(|(k, v)| DataValue::List(vec![k.clone(), DataValue::from(*v)]))
                .collect(),
        ))
    }
}

#[derive(Default)]
pub(crate) struct AggrCountUnique {
    count: i64,
    accum: BTreeSet<DataValue>,
}

impl NormalAggrObj for AggrCountUnique {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        if !self.accum.contains(value) {
            self.accum.insert(value.clone());
            self.count += 1;
        }
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        Ok(DataValue::from(self.count))
    }
}

#[derive(Default)]
pub(crate) struct AggrUnion {
    accum: BTreeSet<DataValue>,
}

impl NormalAggrObj for AggrUnion {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::List(v) => self.accum.extend(v.iter().cloned()),
            v => {
                return TypeMismatchSnafu {
                    op: "union",
                    expected: format!("list, got {v:?}"),
                }
                .fail();
            }
        }
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        Ok(DataValue::List(self.accum.iter().cloned().collect()))
    }
}

pub(crate) struct MeetAggrUnion;

impl MeetAggrObj for MeetAggrUnion {
    fn init_val(&self) -> DataValue {
        DataValue::Set(BTreeSet::new())
    }

    fn update(&self, left: &mut DataValue, right: &DataValue) -> Result<bool> {
        loop {
            if let DataValue::List(l) = left {
                let s = l.iter().cloned().collect();
                *left = DataValue::Set(s);
                continue;
            }
            return Ok(match (left, right) {
                (DataValue::Set(l), DataValue::Set(s)) => {
                    let before = l.len();
                    l.extend(s.iter().cloned());
                    l.len() > before
                }
                (DataValue::Set(l), DataValue::List(s)) => {
                    let before = l.len();
                    l.extend(s.iter().cloned());
                    l.len() > before
                }
                (_, v) => {
                    return TypeMismatchSnafu {
                        op: "union",
                        expected: format!("set or list, got {v:?}"),
                    }
                    .fail();
                }
            });
        }
    }
}

#[derive(Default)]
pub(crate) struct AggrIntersection {
    accum: Option<BTreeSet<DataValue>>,
}

impl NormalAggrObj for AggrIntersection {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::List(v) => {
                if let Some(accum) = &mut self.accum {
                    let new = accum
                        .intersection(&v.iter().cloned().collect())
                        .cloned()
                        .collect();
                    *accum = new;
                } else {
                    self.accum = Some(v.iter().cloned().collect())
                }
            }
            v => {
                return TypeMismatchSnafu {
                    op: "intersection",
                    expected: format!("list, got {v:?}"),
                }
                .fail();
            }
        }
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        match &self.accum {
            None => Ok(DataValue::List(vec![])),
            Some(l) => Ok(DataValue::List(l.iter().cloned().collect())),
        }
    }
}

pub(crate) struct MeetAggrIntersection;

impl MeetAggrObj for MeetAggrIntersection {
    fn init_val(&self) -> DataValue {
        DataValue::Null
    }

    fn update(&self, left: &mut DataValue, right: &DataValue) -> Result<bool> {
        if *left == DataValue::Null && *right != DataValue::Null {
            *left = right.clone();
            return Ok(true);
        } else if *right == DataValue::Null {
            return Ok(false);
        }
        loop {
            if let DataValue::List(l) = left {
                let s = l.iter().cloned().collect();
                *left = DataValue::Set(s);
                continue;
            }
            return Ok(match (left, right) {
                (DataValue::Set(l), DataValue::Set(s)) => {
                    let old_len = l.len();
                    let new_set = l.intersection(s).cloned().collect::<BTreeSet<_>>();
                    if old_len == new_set.len() {
                        false
                    } else {
                        *l = new_set;
                        true
                    }
                }
                (DataValue::Set(l), DataValue::List(s)) => {
                    let old_len = l.len();
                    let s: BTreeSet<_> = s.iter().cloned().collect();
                    let new_set = l.intersection(&s).cloned().collect::<BTreeSet<_>>();
                    if old_len == new_set.len() {
                        false
                    } else {
                        *l = new_set;
                        true
                    }
                }
                (_, v) => {
                    return TypeMismatchSnafu {
                        op: "intersection",
                        expected: format!("set or list, got {v:?}"),
                    }
                    .fail();
                }
            });
        }
    }
}

#[derive(Default)]
pub(crate) struct AggrCollect {
    limit: Option<usize>,
    accum: Vec<DataValue>,
}

impl AggrCollect {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            limit: Some(limit),
            accum: vec![],
        }
    }
}

impl NormalAggrObj for AggrCollect {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        if let Some(limit) = self.limit
            && self.accum.len() >= limit
        {
            return Ok(());
        }
        self.accum.push(value.clone());
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        Ok(DataValue::List(self.accum.clone()))
    }
}

pub(crate) struct AggrChoiceRand {
    count: usize,
    value: DataValue,
}

impl Default for AggrChoiceRand {
    fn default() -> Self {
        Self {
            count: 0,
            value: DataValue::Null,
        }
    }
}

impl NormalAggrObj for AggrChoiceRand {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        self.count += 1;
        let prob = 1. / (self.count as f64);
        let rd = rand::rng().random::<f64>();
        if rd < prob {
            self.value = value.clone();
        }
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        Ok(self.value.clone())
    }
}
