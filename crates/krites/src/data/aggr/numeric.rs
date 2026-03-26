//! Numeric aggregation operators.

use super::super::error::*;

type Result<T> = DataResult<T>;

use crate::data::value::DataValue;

use super::{MeetAggrObj, NormalAggrObj};

#[derive(Default)]
pub(crate) struct AggrCount {
    count: i64,
}

impl NormalAggrObj for AggrCount {
    fn set(&mut self, _value: &DataValue) -> Result<()> {
        self.count += 1;
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        Ok(DataValue::from(self.count))
    }
}

#[derive(Default)]
pub(crate) struct AggrVariance {
    count: i64,
    sum: f64,
    sum_sq: f64,
}

impl NormalAggrObj for AggrVariance {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::Num(n) => {
                let f = n.get_float();
                self.sum += f;
                self.sum_sq += f * f;
                self.count += 1;
            }
            v => {
                return TypeMismatchSnafu {
                    op: "variance",
                    expected: format!("numerical value, got {v:?}"),
                }
                .fail();
            }
        }
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        #[expect(
            clippy::cast_precision_loss,
            reason = "i64 to f64: precision loss acceptable"
        )]
        let ct = self.count as f64;
        Ok(DataValue::from(
            (self.sum_sq - self.sum * self.sum / ct) / (ct - 1.),
        ))
    }
}

#[derive(Default)]
pub(crate) struct AggrStdDev {
    count: i64,
    sum: f64,
    sum_sq: f64,
}

impl NormalAggrObj for AggrStdDev {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::Num(n) => {
                let f = n.get_float();
                self.sum += f;
                self.sum_sq += f * f;
                self.count += 1;
            }
            v => {
                return TypeMismatchSnafu {
                    op: "std_dev",
                    expected: format!("numerical value, got {v:?}"),
                }
                .fail();
            }
        }
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        #[expect(
            clippy::cast_precision_loss,
            reason = "i64 to f64: precision loss acceptable"
        )]
        let ct = self.count as f64;
        let var = (self.sum_sq - self.sum * self.sum / ct) / (ct - 1.);
        Ok(DataValue::from(var.sqrt()))
    }
}

#[derive(Default)]
pub(crate) struct AggrMean {
    count: i64,
    sum: f64,
}

impl NormalAggrObj for AggrMean {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::Num(n) => {
                self.sum += n.get_float();
                self.count += 1;
            }
            v => {
                return TypeMismatchSnafu {
                    op: "mean",
                    expected: format!("numerical value, got {v:?}"),
                }
                .fail();
            }
        }
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        Ok(DataValue::from(self.sum / (self.count as f64)))
    }
}

#[derive(Default)]
pub(crate) struct AggrSum {
    sum: f64,
}

impl NormalAggrObj for AggrSum {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::Num(n) => {
                self.sum += n.get_float();
            }
            v => {
                return TypeMismatchSnafu {
                    op: "sum",
                    expected: format!("numerical value, got {v:?}"),
                }
                .fail();
            }
        }
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        Ok(DataValue::from(self.sum))
    }
}

pub(crate) struct AggrProduct {
    product: f64,
}

impl Default for AggrProduct {
    fn default() -> Self {
        Self { product: 1.0 }
    }
}

impl NormalAggrObj for AggrProduct {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::Num(n) => {
                self.product *= n.get_float();
            }
            v => {
                return TypeMismatchSnafu {
                    op: "product",
                    expected: format!("numerical value, got {v:?}"),
                }
                .fail();
            }
        }
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        Ok(DataValue::from(self.product))
    }
}

pub(crate) struct AggrMin {
    found: DataValue,
}

impl Default for AggrMin {
    fn default() -> Self {
        Self {
            found: DataValue::Null,
        }
    }
}

impl NormalAggrObj for AggrMin {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        if *value == DataValue::Null {
            return Ok(());
        }
        if self.found == DataValue::Null {
            self.found = value.clone();
            return Ok(());
        }
        let f1 = self.found.get_float().ok_or_else(|| {
            TypeMismatchSnafu {
                op: "min",
                expected: "numerical values",
            }
            .build()
        })?;
        let f2 = value.get_float().ok_or_else(|| {
            TypeMismatchSnafu {
                op: "min",
                expected: "numerical values",
            }
            .build()
        })?;
        if f1 > f2 {
            self.found = value.clone();
        }
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        Ok(self.found.clone())
    }
}

pub(crate) struct MeetAggrMin;

impl MeetAggrObj for MeetAggrMin {
    fn init_val(&self) -> DataValue {
        DataValue::Null
    }

    fn update(&self, left: &mut DataValue, right: &DataValue) -> Result<bool> {
        if *right == DataValue::Null {
            return Ok(false);
        }
        if *left == DataValue::Null {
            *left = right.clone();
            return Ok(true);
        }
        let f1 = left.get_float().ok_or_else(|| {
            TypeMismatchSnafu {
                op: "min",
                expected: "numerical values",
            }
            .build()
        })?;
        let f2 = right.get_float().ok_or_else(|| {
            TypeMismatchSnafu {
                op: "min",
                expected: "numerical values",
            }
            .build()
        })?;

        Ok(if f1 > f2 {
            *left = right.clone();
            true
        } else {
            false
        })
    }
}

pub(crate) struct AggrMax {
    found: DataValue,
}

impl Default for AggrMax {
    fn default() -> Self {
        Self {
            found: DataValue::Null,
        }
    }
}

impl NormalAggrObj for AggrMax {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        if *value == DataValue::Null {
            return Ok(());
        }
        if self.found == DataValue::Null {
            self.found = value.clone();
            return Ok(());
        }
        let f1 = self.found.get_float().ok_or_else(|| {
            TypeMismatchSnafu {
                op: "max",
                expected: "numerical values",
            }
            .build()
        })?;
        let f2 = value.get_float().ok_or_else(|| {
            TypeMismatchSnafu {
                op: "max",
                expected: "numerical values",
            }
            .build()
        })?;
        if f1 < f2 {
            self.found = value.clone();
        }
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        Ok(self.found.clone())
    }
}

pub(crate) struct MeetAggrMax;

impl MeetAggrObj for MeetAggrMax {
    fn init_val(&self) -> DataValue {
        DataValue::Null
    }

    fn update(&self, left: &mut DataValue, right: &DataValue) -> Result<bool> {
        if *right == DataValue::Null {
            return Ok(false);
        }
        if *left == DataValue::Null {
            *left = right.clone();
            return Ok(true);
        }
        let f1 = left.get_float().ok_or_else(|| {
            TypeMismatchSnafu {
                op: "max",
                expected: "numerical values",
            }
            .build()
        })?;
        let f2 = right.get_float().ok_or_else(|| {
            TypeMismatchSnafu {
                op: "max",
                expected: "numerical values",
            }
            .build()
        })?;

        Ok(if f1 < f2 {
            *left = right.clone();
            true
        } else {
            false
        })
    }
}

pub(crate) struct AggrLatestBy {
    found: DataValue,
    cost: DataValue,
}

impl Default for AggrLatestBy {
    fn default() -> Self {
        Self {
            found: DataValue::Null,
            cost: DataValue::Null,
        }
    }
}

impl NormalAggrObj for AggrLatestBy {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::List(l) => {
                snafu::ensure!(
                    l.len() == 2,
                    InvalidValueSnafu {
                        message: "'latest_by' requires a list of exactly two items as argument"
                    }
                );
                #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
                let c = &l[1];
                if *c > self.cost {
                    self.cost = c.clone();
                    self.found = l[0].clone();
                }
                Ok(())
            }
            v => TypeMismatchSnafu {
                op: "latest_by",
                expected: format!("list, got {v:?}"),
            }
            .fail(),
        }
    }

    fn get(&self) -> Result<DataValue> {
        Ok(self.found.clone())
    }
}

pub(crate) struct AggrSmallestBy {
    found: DataValue,
    cost: DataValue,
}

impl Default for AggrSmallestBy {
    fn default() -> Self {
        Self {
            found: DataValue::Null,
            cost: DataValue::Null,
        }
    }
}

impl NormalAggrObj for AggrSmallestBy {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::List(l) => {
                snafu::ensure!(
                    l.len() == 2,
                    InvalidValueSnafu {
                        message: "'smallest_by' requires a list of exactly two items as argument"
                    }
                );
                #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
                let c = &l[1];
                if self.cost == DataValue::Null || *c < self.cost {
                    self.cost = c.clone();
                    self.found = l[0].clone();
                }
                Ok(())
            }
            v => TypeMismatchSnafu {
                op: "smallest_by",
                expected: format!("list, got {v:?}"),
            }
            .fail(),
        }
    }

    fn get(&self) -> Result<DataValue> {
        Ok(self.found.clone())
    }
}

pub(crate) struct AggrMinCost {
    found: DataValue,
    cost: f64,
}

impl Default for AggrMinCost {
    fn default() -> Self {
        Self {
            found: DataValue::Null,
            cost: f64::INFINITY,
        }
    }
}

impl NormalAggrObj for AggrMinCost {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::List(l) => {
                snafu::ensure!(
                    l.len() == 2,
                    InvalidValueSnafu {
                        message: "'min_cost' requires a list of exactly two items as argument"
                    }
                );
                #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
                let c = &l[1];
                let cost = c.get_float().ok_or_else(|| {
                    TypeMismatchSnafu {
                        op: "min_cost",
                        expected: "numerical cost",
                    }
                    .build()
                })?;
                if cost < self.cost {
                    self.cost = cost;
                    self.found = l[0].clone();
                }
                Ok(())
            }
            v => TypeMismatchSnafu {
                op: "min_cost",
                expected: format!("list, got {v:?}"),
            }
            .fail(),
        }
    }

    fn get(&self) -> Result<DataValue> {
        Ok(DataValue::List(vec![
            self.found.clone(),
            DataValue::from(self.cost),
        ]))
    }
}

pub(crate) struct MeetAggrMinCost;

impl MeetAggrObj for MeetAggrMinCost {
    fn init_val(&self) -> DataValue {
        DataValue::List(vec![DataValue::Null, DataValue::from(f64::INFINITY)])
    }

    fn update(&self, left: &mut DataValue, right: &DataValue) -> Result<bool> {
        Ok(match (left, right) {
            (DataValue::List(prev), DataValue::List(l)) => {
                snafu::ensure!(
                    l.len() == 2 && prev.len() == 2,
                    InvalidValueSnafu {
                        message: format!(
                            "'min_cost' requires a list of length 2 as argument, got {prev:?}, {l:?}"
                        )
                    }
                );
                #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
                let cur_cost = &l[1];
                let cur_cost = cur_cost.get_float().ok_or_else(|| {
                    TypeMismatchSnafu {
                        op: "min_cost",
                        expected: "numerical costs",
                    }
                    .build()
                })?;
                #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
                let prev_cost = &prev[1];
                let prev_cost = prev_cost.get_float().ok_or_else(|| {
                    TypeMismatchSnafu {
                        op: "min_cost",
                        expected: "numerical costs",
                    }
                    .build()
                })?;

                if prev_cost <= cur_cost {
                    false
                } else {
                    *prev = l.clone();
                    true
                }
            }
            (u, v) => {
                return TypeMismatchSnafu {
                    op: "min_cost",
                    expected: format!("list, got {u:?} and {v:?}"),
                }
                .fail();
            }
        })
    }
}
