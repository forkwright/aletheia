//! Aggregation operators for Datalog queries.
use std::fmt::{Debug, Formatter};

use super::error::*;

type Result<T> = DataResult<T>;

use crate::data::value::DataValue;

macro_rules! define_aggr {
    ($name:ident, $is_meet:expr) => {
        const $name: Aggregation = Aggregation {
            name: stringify!($name),
            is_meet: $is_meet,
            meet_op: None,
            normal_op: None,
        };
    };
}

mod boolean;
mod misc;
mod numeric;

pub(crate) use boolean::{
    AggrAnd, AggrChoiceRand, AggrCollect, AggrCountUnique, AggrGroupCount, AggrIntersection,
    AggrOr, AggrUnion, AggrUnique, MeetAggrAnd, MeetAggrIntersection, MeetAggrOr, MeetAggrUnion,
};
pub(crate) use misc::{
    AggrBitAnd, AggrBitOr, AggrBitXor, AggrChoice, AggrShortest, MeetAggrBitAnd, MeetAggrBitOr,
    MeetAggrChoice, MeetAggrShortest,
};
pub(crate) use numeric::{
    AggrCount, AggrLatestBy, AggrMax, AggrMean, AggrMin, AggrMinCost, AggrProduct, AggrSmallestBy,
    AggrStdDev, AggrSum, AggrVariance, MeetAggrMax, MeetAggrMin, MeetAggrMinCost,
};

pub(crate) struct Aggregation {
    pub(crate) name: &'static str,
    pub(crate) is_meet: bool,
    pub(crate) meet_op: Option<Box<dyn MeetAggrObj>>,
    pub(crate) normal_op: Option<Box<dyn NormalAggrObj>>,
}

impl Clone for Aggregation {
    fn clone(&self) -> Self {
        Self {
            name: self.name,
            is_meet: self.is_meet,
            meet_op: None,
            normal_op: None,
        }
    }
}

pub(crate) trait NormalAggrObj: Send + Sync {
    fn set(&mut self, value: &DataValue) -> Result<()>;
    fn get(&self) -> Result<DataValue>;
}

pub(crate) trait MeetAggrObj: Send + Sync {
    fn init_val(&self) -> DataValue;
    fn update(&self, left: &mut DataValue, right: &DataValue) -> Result<bool>;
}

impl PartialEq for Aggregation {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Debug for Aggregation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Aggr<{}>", self.name)
    }
}

define_aggr!(AGGR_AND, true);
define_aggr!(AGGR_OR, true);
define_aggr!(AGGR_UNIQUE, false);
define_aggr!(AGGR_GROUP_COUNT, false);
define_aggr!(AGGR_COUNT_UNIQUE, false);
define_aggr!(AGGR_UNION, true);
define_aggr!(AGGR_INTERSECTION, true);
define_aggr!(AGGR_COLLECT, false);
define_aggr!(AGGR_CHOICE_RAND, false);
define_aggr!(AGGR_COUNT, false);
define_aggr!(AGGR_VARIANCE, false);
define_aggr!(AGGR_STD_DEV, false);
define_aggr!(AGGR_MEAN, false);
define_aggr!(AGGR_SUM, false);
define_aggr!(AGGR_PRODUCT, false);
define_aggr!(AGGR_MIN, true);
define_aggr!(AGGR_MAX, true);
define_aggr!(AGGR_LATEST_BY, false);
define_aggr!(AGGR_SMALLEST_BY, false);
define_aggr!(AGGR_MIN_COST, true);
define_aggr!(AGGR_SHORTEST, true);
define_aggr!(AGGR_CHOICE, true);
define_aggr!(AGGR_BIT_AND, true);
define_aggr!(AGGR_BIT_OR, true);
define_aggr!(AGGR_BIT_XOR, false);

pub(crate) fn parse_aggr(name: &str) -> Option<&'static Aggregation> {
    Some(match name {
        "and" => &AGGR_AND,
        "or" => &AGGR_OR,
        "unique" => &AGGR_UNIQUE,
        "group_count" => &AGGR_GROUP_COUNT,
        "union" => &AGGR_UNION,
        "intersection" => &AGGR_INTERSECTION,
        "count" => &AGGR_COUNT,
        "count_unique" => &AGGR_COUNT_UNIQUE,
        "variance" => &AGGR_VARIANCE,
        "std_dev" => &AGGR_STD_DEV,
        "sum" => &AGGR_SUM,
        "product" => &AGGR_PRODUCT,
        "min" => &AGGR_MIN,
        "max" => &AGGR_MAX,
        "mean" => &AGGR_MEAN,
        "choice" => &AGGR_CHOICE,
        "collect" => &AGGR_COLLECT,
        "shortest" => &AGGR_SHORTEST,
        "min_cost" => &AGGR_MIN_COST,
        "bit_and" => &AGGR_BIT_AND,
        "bit_or" => &AGGR_BIT_OR,
        "bit_xor" => &AGGR_BIT_XOR,
        "latest_by" => &AGGR_LATEST_BY,
        "smallest_by" => &AGGR_SMALLEST_BY,
        "choice_rand" => &AGGR_CHOICE_RAND,
        _ => return None,
    })
}

impl Aggregation {
    pub(crate) fn meet_init(&mut self, _args: &[DataValue]) -> Result<()> {
        self.meet_op.replace(match self.name {
            name if name == AGGR_AND.name => Box::new(MeetAggrAnd),
            name if name == AGGR_OR.name => Box::new(MeetAggrOr),
            name if name == AGGR_MIN.name => Box::new(MeetAggrMin),
            name if name == AGGR_MAX.name => Box::new(MeetAggrMax),
            name if name == AGGR_CHOICE.name => Box::new(MeetAggrChoice),
            name if name == AGGR_BIT_AND.name => Box::new(MeetAggrBitAnd),
            name if name == AGGR_BIT_OR.name => Box::new(MeetAggrBitOr),
            name if name == AGGR_UNION.name => Box::new(MeetAggrUnion),
            name if name == AGGR_INTERSECTION.name => Box::new(MeetAggrIntersection),
            name if name == AGGR_SHORTEST.name => Box::new(MeetAggrShortest),
            name if name == AGGR_MIN_COST.name => Box::new(MeetAggrMinCost),
            name => unreachable!("{}", name),
        });
        Ok(())
    }
    pub(crate) fn normal_init(&mut self, args: &[DataValue]) -> Result<()> {
        self.normal_op.replace(match self.name {
            name if name == AGGR_AND.name => Box::new(AggrAnd::default()),
            name if name == AGGR_OR.name => Box::new(AggrOr::default()),
            name if name == AGGR_COUNT.name => Box::new(AggrCount::default()),
            name if name == AGGR_GROUP_COUNT.name => Box::new(AggrGroupCount::default()),
            name if name == AGGR_COUNT_UNIQUE.name => Box::new(AggrCountUnique::default()),
            name if name == AGGR_SUM.name => Box::new(AggrSum::default()),
            name if name == AGGR_PRODUCT.name => Box::new(AggrProduct::default()),
            name if name == AGGR_MIN.name => Box::new(AggrMin::default()),
            name if name == AGGR_MAX.name => Box::new(AggrMax::default()),
            name if name == AGGR_MEAN.name => Box::new(AggrMean::default()),
            name if name == AGGR_VARIANCE.name => Box::new(AggrVariance::default()),
            name if name == AGGR_STD_DEV.name => Box::new(AggrStdDev::default()),
            name if name == AGGR_CHOICE.name => Box::new(AggrChoice::default()),
            name if name == AGGR_BIT_AND.name => Box::new(AggrBitAnd::default()),
            name if name == AGGR_BIT_OR.name => Box::new(AggrBitOr::default()),
            name if name == AGGR_BIT_XOR.name => Box::new(AggrBitXor::default()),
            name if name == AGGR_UNIQUE.name => Box::new(AggrUnique::default()),
            name if name == AGGR_UNION.name => Box::new(AggrUnion::default()),
            name if name == AGGR_INTERSECTION.name => Box::new(AggrIntersection::default()),
            name if name == AGGR_SHORTEST.name => Box::new(AggrShortest::default()),
            name if name == AGGR_MIN_COST.name => Box::new(AggrMinCost::default()),
            name if name == AGGR_LATEST_BY.name => Box::new(AggrLatestBy::default()),
            name if name == AGGR_SMALLEST_BY.name => Box::new(AggrSmallestBy::default()),
            name if name == AGGR_CHOICE_RAND.name => Box::new(AggrChoiceRand::default()),
            name if name == AGGR_COLLECT.name => Box::new({
                if args.is_empty() {
                    AggrCollect::default()
                } else {
                    #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
                    let arg = args[0].get_int().ok_or_else(|| {
                        TypeMismatchSnafu {
                            op: "collect",
                            expected: format!("integer argument, got {:?}", args[0]),
                        }
                        .build()
                    })?;
                    snafu::ensure!(
                        arg > 0,
                        InvalidValueSnafu {
                            message: format!("argument to 'collect' must be positive, got {arg}")
                        }
                    );
                    AggrCollect::new(arg as usize)
                }
            }),
            _ => unreachable!(),
        });
        Ok(())
    }
}
