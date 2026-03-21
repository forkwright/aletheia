//! Miscellaneous aggregation operators: path, choice, bitwise.
use super::super::error::*;

type Result<T> = DataResult<T>;

use crate::data::value::DataValue;

use super::{MeetAggrObj, NormalAggrObj};

#[derive(Default)]
pub(crate) struct AggrShortest {
    found: Option<Vec<DataValue>>,
}

impl NormalAggrObj for AggrShortest {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::List(l) => {
                match self.found {
                    None => self.found = Some(l.clone()),
                    Some(ref mut found) => {
                        if l.len() < found.len() {
                            *found = l.clone();
                        }
                    }
                }
                Ok(())
            }
            v => TypeMismatchSnafu {
                op: "shortest",
                expected: format!("list, got {v:?}"),
            }
            .fail(),
        }
    }

    fn get(&self) -> Result<DataValue> {
        Ok(match self.found {
            None => DataValue::Null,
            Some(ref l) => DataValue::List(l.clone()),
        })
    }
}

pub(crate) struct MeetAggrShortest;

impl MeetAggrObj for MeetAggrShortest {
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
        match (left, right) {
            (DataValue::List(l), DataValue::List(r)) => Ok(if r.len() < l.len() {
                *l = r.clone();
                true
            } else {
                false
            }),
            (l, v) => TypeMismatchSnafu {
                op: "shortest",
                expected: format!("list, got {l:?} and {v:?}"),
            }
            .fail(),
        }
    }
}

pub(crate) struct AggrChoice {
    found: DataValue,
}

impl Default for AggrChoice {
    fn default() -> Self {
        Self {
            found: DataValue::Null,
        }
    }
}

impl NormalAggrObj for AggrChoice {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        if self.found == DataValue::Null {
            self.found = value.clone();
        }
        Ok(())
    }

    fn get(&self) -> Result<DataValue> {
        Ok(self.found.clone())
    }
}

pub(crate) struct MeetAggrChoice;

impl MeetAggrObj for MeetAggrChoice {
    fn init_val(&self) -> DataValue {
        DataValue::Null
    }

    fn update(&self, left: &mut DataValue, right: &DataValue) -> Result<bool> {
        Ok(if *left == DataValue::Null && *right != DataValue::Null {
            *left = right.clone();
            true
        } else {
            false
        })
    }
}

#[derive(Default)]
pub(crate) struct AggrBitAnd {
    res: Vec<u8>,
}

impl NormalAggrObj for AggrBitAnd {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::Bytes(bs) => {
                if self.res.is_empty() {
                    self.res = bs.to_vec();
                } else {
                    snafu::ensure!(
                        self.res.len() == bs.len(),
                        ByteLengthMismatchSnafu { op: "bit_and" }
                    );
                    for (l, r) in self.res.iter_mut().zip(bs.iter()) {
                        *l &= *r;
                    }
                }
                Ok(())
            }
            v => TypeMismatchSnafu {
                op: "bit_and",
                expected: format!("bytes, got {v:?}"),
            }
            .fail(),
        }
    }

    fn get(&self) -> Result<DataValue> {
        Ok(DataValue::Bytes(self.res.clone()))
    }
}

pub(crate) struct MeetAggrBitAnd;

impl MeetAggrObj for MeetAggrBitAnd {
    fn init_val(&self) -> DataValue {
        DataValue::Bytes(vec![])
    }

    fn update(&self, left: &mut DataValue, right: &DataValue) -> Result<bool> {
        match (left, right) {
            (DataValue::Bytes(left), DataValue::Bytes(right)) => {
                if left == right {
                    return Ok(false);
                }
                if left.is_empty() {
                    *left = right.clone();
                    return Ok(true);
                }
                snafu::ensure!(
                    left.len() == right.len(),
                    ByteLengthMismatchSnafu { op: "bit_and" }
                );
                for (l, r) in left.iter_mut().zip(right.iter()) {
                    *l &= *r;
                }

                Ok(true)
            }
            v => TypeMismatchSnafu {
                op: "bit_and",
                expected: format!("bytes, got {v:?}"),
            }
            .fail(),
        }
    }
}

#[derive(Default)]
pub(crate) struct AggrBitOr {
    res: Vec<u8>,
}

impl NormalAggrObj for AggrBitOr {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::Bytes(bs) => {
                if self.res.is_empty() {
                    self.res = bs.to_vec();
                } else {
                    snafu::ensure!(
                        self.res.len() == bs.len(),
                        ByteLengthMismatchSnafu { op: "bit_or" }
                    );
                    for (l, r) in self.res.iter_mut().zip(bs.iter()) {
                        *l |= *r;
                    }
                }
                Ok(())
            }
            v => TypeMismatchSnafu {
                op: "bit_or",
                expected: format!("bytes, got {v:?}"),
            }
            .fail(),
        }
    }

    fn get(&self) -> Result<DataValue> {
        Ok(DataValue::Bytes(self.res.clone()))
    }
}

pub(crate) struct MeetAggrBitOr;

impl MeetAggrObj for MeetAggrBitOr {
    fn init_val(&self) -> DataValue {
        DataValue::Bytes(vec![])
    }

    fn update(&self, left: &mut DataValue, right: &DataValue) -> Result<bool> {
        match (left, right) {
            (DataValue::Bytes(left), DataValue::Bytes(right)) => {
                if left == right {
                    return Ok(false);
                }
                if left.is_empty() {
                    *left = right.clone();
                    return Ok(true);
                }
                snafu::ensure!(
                    left.len() == right.len(),
                    ByteLengthMismatchSnafu { op: "bit_or" }
                );
                for (l, r) in left.iter_mut().zip(right.iter()) {
                    *l |= *r;
                }

                Ok(true)
            }
            v => TypeMismatchSnafu {
                op: "bit_or",
                expected: format!("bytes, got {v:?}"),
            }
            .fail(),
        }
    }
}

#[derive(Default)]
pub(crate) struct AggrBitXor {
    res: Vec<u8>,
}

impl NormalAggrObj for AggrBitXor {
    fn set(&mut self, value: &DataValue) -> Result<()> {
        match value {
            DataValue::Bytes(bs) => {
                if self.res.is_empty() {
                    self.res = bs.to_vec();
                } else {
                    snafu::ensure!(
                        self.res.len() == bs.len(),
                        ByteLengthMismatchSnafu { op: "bit_xor" }
                    );
                    for (l, r) in self.res.iter_mut().zip(bs.iter()) {
                        *l ^= *r;
                    }
                }
                Ok(())
            }
            v => TypeMismatchSnafu {
                op: "bit_xor",
                expected: format!("bytes, got {v:?}"),
            }
            .fail(),
        }
    }

    fn get(&self) -> Result<DataValue> {
        Ok(DataValue::Bytes(self.res.clone()))
    }
}
