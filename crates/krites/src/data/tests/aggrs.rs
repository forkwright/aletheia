//! Tests for aggregation operators.
#![expect(clippy::expect_used, reason = "test assertions")]
use itertools::Itertools;

use crate::data::aggr::parse_aggr;
use crate::data::value::DataValue;

#[test]
fn test_and() {
    let mut aggr = parse_aggr("and").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");
    aggr.meet_init(&[]).expect("test assertion");
    let mut and_aggr = aggr.normal_op.expect("test assertion");
    assert_eq!(
        and_aggr.get().expect("test assertion"),
        DataValue::from(true)
    );

    and_aggr
        .set(&DataValue::from(true))
        .expect("test assertion");
    and_aggr
        .set(&DataValue::from(true))
        .expect("test assertion");

    assert_eq!(
        and_aggr.get().expect("test assertion"),
        DataValue::from(true)
    );
    and_aggr
        .set(&DataValue::from(false))
        .expect("test assertion");

    assert_eq!(
        and_aggr.get().expect("test assertion"),
        DataValue::from(false)
    );

    let m_and_aggr = aggr.meet_op.expect("test assertion");
    let mut v = DataValue::from(true);

    m_and_aggr
        .update(&mut v, &DataValue::from(true))
        .expect("test assertion");
    assert_eq!(v, DataValue::from(true));

    m_and_aggr
        .update(&mut v, &DataValue::from(false))
        .expect("test assertion");
    assert_eq!(v, DataValue::from(false));

    m_and_aggr
        .update(&mut v, &DataValue::from(true))
        .expect("test assertion");
    assert_eq!(v, DataValue::from(false));
}

#[test]
fn test_or() {
    let mut aggr = parse_aggr("or").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");
    aggr.meet_init(&[]).expect("test assertion");

    let mut or_aggr = aggr.normal_op.expect("test assertion");
    assert_eq!(
        or_aggr.get().expect("test assertion"),
        DataValue::from(false)
    );

    or_aggr
        .set(&DataValue::from(false))
        .expect("test assertion");
    or_aggr
        .set(&DataValue::from(false))
        .expect("test assertion");

    assert_eq!(
        or_aggr.get().expect("test assertion"),
        DataValue::from(false)
    );
    or_aggr.set(&DataValue::from(true)).expect("test assertion");

    assert_eq!(
        or_aggr.get().expect("test assertion"),
        DataValue::from(true)
    );

    let m_or_aggr = aggr.meet_op.expect("test assertion");
    let mut v = DataValue::from(false);

    m_or_aggr
        .update(&mut v, &DataValue::from(false))
        .expect("test assertion");
    assert_eq!(v, DataValue::from(false));

    m_or_aggr
        .update(&mut v, &DataValue::from(true))
        .expect("test assertion");
    assert_eq!(v, DataValue::from(true));

    m_or_aggr
        .update(&mut v, &DataValue::from(false))
        .expect("test assertion");
    assert_eq!(v, DataValue::from(true));
}

#[test]
fn test_unique() {
    let mut aggr = parse_aggr("unique").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");
    let mut unique_aggr = aggr.normal_op.expect("test assertion");

    unique_aggr
        .set(&DataValue::from(true))
        .expect("test assertion");
    unique_aggr
        .set(&DataValue::from(1))
        .expect("test assertion");
    unique_aggr
        .set(&DataValue::from(2))
        .expect("test assertion");
    unique_aggr
        .set(&DataValue::from(1))
        .expect("test assertion");
    assert_eq!(
        unique_aggr.get().expect("test assertion"),
        DataValue::List(vec![
            DataValue::from(true),
            DataValue::from(1),
            DataValue::from(2),
        ])
    );
}

#[test]
fn test_group_count() {
    let mut aggr = parse_aggr("group_count").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");

    let mut group_count_aggr = aggr.normal_op.expect("test assertion");
    group_count_aggr
        .set(&DataValue::from(1.))
        .expect("test assertion");
    group_count_aggr
        .set(&DataValue::from(2.))
        .expect("test assertion");
    group_count_aggr
        .set(&DataValue::from(3.))
        .expect("test assertion");
    group_count_aggr
        .set(&DataValue::from(3.))
        .expect("test assertion");
    group_count_aggr
        .set(&DataValue::from(1.))
        .expect("test assertion");
    group_count_aggr
        .set(&DataValue::from(3.))
        .expect("test assertion");
    assert_eq!(
        group_count_aggr.get().expect("test assertion"),
        DataValue::List(vec![
            DataValue::List(vec![DataValue::from(1.), DataValue::from(2)]),
            DataValue::List(vec![DataValue::from(2.), DataValue::from(1)]),
            DataValue::List(vec![DataValue::from(3.), DataValue::from(3)]),
        ])
    )
}

#[test]
fn test_union() {
    let mut aggr = parse_aggr("union").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");
    aggr.meet_init(&[]).expect("test assertion");

    let mut union_aggr = aggr.normal_op.expect("test assertion");
    union_aggr
        .set(&DataValue::List(
            [1, 3, 5, 2].into_iter().map(DataValue::from).collect_vec(),
        ))
        .expect("test assertion");
    union_aggr
        .set(&DataValue::List(
            [10, 2, 4, 6].into_iter().map(DataValue::from).collect_vec(),
        ))
        .expect("test assertion");
    assert_eq!(
        union_aggr.get().expect("test assertion"),
        DataValue::List(
            [1, 2, 3, 4, 5, 6, 10]
                .into_iter()
                .map(DataValue::from)
                .collect_vec()
        )
    );
    let mut v = DataValue::List([1, 3, 5, 2].into_iter().map(DataValue::from).collect_vec());

    let m_aggr_union = aggr.meet_op.expect("test assertion");
    m_aggr_union
        .update(
            &mut v,
            &DataValue::List([10, 2, 4, 6].into_iter().map(DataValue::from).collect_vec()),
        )
        .expect("test assertion");
    assert_eq!(
        v,
        DataValue::Set(
            [1, 2, 3, 4, 5, 6, 10]
                .into_iter()
                .map(DataValue::from)
                .collect()
        )
    );
}

#[test]
fn test_intersection() {
    let mut aggr = parse_aggr("intersection").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");
    aggr.meet_init(&[]).expect("test assertion");

    let mut intersection_aggr = aggr.normal_op.expect("test assertion");
    intersection_aggr
        .set(&DataValue::List(
            [1, 3, 5, 2].into_iter().map(DataValue::from).collect_vec(),
        ))
        .expect("test assertion");
    intersection_aggr
        .set(&DataValue::List(
            [10, 2, 4, 6].into_iter().map(DataValue::from).collect_vec(),
        ))
        .expect("test assertion");
    assert_eq!(
        intersection_aggr.get().expect("test assertion"),
        DataValue::List([2].into_iter().map(DataValue::from).collect_vec())
    );
    let mut v = DataValue::List([1, 3, 5, 2].into_iter().map(DataValue::from).collect_vec());

    let m_aggr_intersection = aggr.meet_op.expect("test assertion");
    m_aggr_intersection
        .update(
            &mut v,
            &DataValue::List([10, 2, 4, 6].into_iter().map(DataValue::from).collect_vec()),
        )
        .expect("test assertion");
    assert_eq!(
        v,
        DataValue::Set([2].into_iter().map(DataValue::from).collect())
    );
}

#[test]
fn test_count_unique() {
    let mut aggr = parse_aggr("count_unique").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");

    let mut count_unique_aggr = aggr.normal_op.expect("test assertion");
    count_unique_aggr
        .set(&DataValue::from(1))
        .expect("test assertion");
    count_unique_aggr
        .set(&DataValue::from(2))
        .expect("test assertion");
    count_unique_aggr
        .set(&DataValue::from(3))
        .expect("test assertion");
    count_unique_aggr
        .set(&DataValue::from(1))
        .expect("test assertion");
    count_unique_aggr
        .set(&DataValue::from(2))
        .expect("test assertion");
    count_unique_aggr
        .set(&DataValue::from(1))
        .expect("test assertion");
    assert_eq!(
        count_unique_aggr.get().expect("test assertion"),
        DataValue::from(3)
    );
}

#[test]
fn test_collect() {
    let mut aggr = parse_aggr("collect").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");

    let mut collect_aggr = aggr.normal_op.expect("test assertion");
    collect_aggr
        .set(&DataValue::from(1))
        .expect("test assertion");
    collect_aggr
        .set(&DataValue::from(2))
        .expect("test assertion");
    collect_aggr
        .set(&DataValue::from(3))
        .expect("test assertion");
    collect_aggr
        .set(&DataValue::from(1))
        .expect("test assertion");
    collect_aggr
        .set(&DataValue::from(2))
        .expect("test assertion");
    collect_aggr
        .set(&DataValue::from(1))
        .expect("test assertion");
    assert_eq!(
        collect_aggr.get().expect("test assertion"),
        DataValue::List(
            [1, 2, 3, 1, 2, 1]
                .into_iter()
                .map(DataValue::from)
                .collect()
        )
    );
}

#[test]
fn test_count() {
    let mut aggr = parse_aggr("count").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");

    let mut count_aggr = aggr.normal_op.expect("test assertion");
    count_aggr.set(&DataValue::Null).expect("test assertion");
    count_aggr.set(&DataValue::Null).expect("test assertion");
    count_aggr.set(&DataValue::Null).expect("test assertion");
    count_aggr.set(&DataValue::Null).expect("test assertion");
    count_aggr
        .set(&DataValue::from(true))
        .expect("test assertion");
    count_aggr
        .set(&DataValue::from(true))
        .expect("test assertion");
    assert_eq!(
        count_aggr.get().expect("test assertion"),
        DataValue::from(6)
    );
}

#[test]
fn test_variance() {
    let mut aggr = parse_aggr("variance").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");

    let mut variance_aggr = aggr.normal_op.expect("test assertion");
    variance_aggr
        .set(&DataValue::from(1))
        .expect("test assertion");
    variance_aggr
        .set(&DataValue::from(2))
        .expect("test assertion");
    assert_eq!(
        variance_aggr.get().expect("test assertion"),
        DataValue::from(0.5)
    )
}

#[test]
fn test_std_dev() {
    let mut aggr = parse_aggr("std_dev").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");

    let mut std_dev_aggr = aggr.normal_op.expect("test assertion");
    std_dev_aggr
        .set(&DataValue::from(1))
        .expect("test assertion");
    std_dev_aggr
        .set(&DataValue::from(2))
        .expect("test assertion");
    let v = std_dev_aggr
        .get()
        .expect("test assertion")
        .get_float()
        .expect("test assertion");
    assert!((v - (0.5_f64).sqrt()).abs() < 1e-10);
}

#[test]
fn test_mean() {
    let mut aggr = parse_aggr("mean").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");

    let mut mean_aggr = aggr.normal_op.expect("test assertion");
    mean_aggr.set(&DataValue::from(1)).expect("test assertion");
    mean_aggr.set(&DataValue::from(2)).expect("test assertion");
    mean_aggr.set(&DataValue::from(3)).expect("test assertion");
    mean_aggr.set(&DataValue::from(4)).expect("test assertion");
    mean_aggr.set(&DataValue::from(5)).expect("test assertion");
    assert_eq!(
        mean_aggr.get().expect("test assertion"),
        DataValue::from(3.)
    );
}

#[test]
fn test_sum() {
    let mut aggr = parse_aggr("sum").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");

    let mut sum_aggr = aggr.normal_op.expect("test assertion");
    sum_aggr.set(&DataValue::from(1)).expect("test assertion");
    sum_aggr.set(&DataValue::from(2)).expect("test assertion");
    sum_aggr.set(&DataValue::from(3)).expect("test assertion");
    sum_aggr.set(&DataValue::from(4)).expect("test assertion");
    sum_aggr.set(&DataValue::from(5)).expect("test assertion");
    assert_eq!(
        sum_aggr.get().expect("test assertion"),
        DataValue::from(15.)
    );
}

#[test]
fn test_product() {
    let mut aggr = parse_aggr("product").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");

    let mut product_aggr = aggr.normal_op.expect("test assertion");
    product_aggr
        .set(&DataValue::from(1))
        .expect("test assertion");
    product_aggr
        .set(&DataValue::from(2))
        .expect("test assertion");
    product_aggr
        .set(&DataValue::from(3))
        .expect("test assertion");
    product_aggr
        .set(&DataValue::from(4))
        .expect("test assertion");
    product_aggr
        .set(&DataValue::from(5))
        .expect("test assertion");
    assert_eq!(
        product_aggr.get().expect("test assertion"),
        DataValue::from(120.)
    );
}

#[test]
fn test_min() {
    let mut aggr = parse_aggr("min").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");
    aggr.meet_init(&[]).expect("test assertion");

    let mut min_aggr = aggr.normal_op.expect("test assertion");
    min_aggr.set(&DataValue::from(10)).expect("test assertion");
    min_aggr.set(&DataValue::from(9)).expect("test assertion");
    min_aggr.set(&DataValue::from(1)).expect("test assertion");
    min_aggr.set(&DataValue::from(2)).expect("test assertion");
    min_aggr.set(&DataValue::from(3)).expect("test assertion");
    assert_eq!(min_aggr.get().expect("test assertion"), DataValue::from(1));

    let m_min_aggr = aggr.meet_op.expect("test assertion");
    let mut v = DataValue::from(5);
    m_min_aggr
        .update(&mut v, &DataValue::from(10))
        .expect("test assertion");
    m_min_aggr
        .update(&mut v, &DataValue::from(9))
        .expect("test assertion");
    m_min_aggr
        .update(&mut v, &DataValue::from(1))
        .expect("test assertion");
    m_min_aggr
        .update(&mut v, &DataValue::from(2))
        .expect("test assertion");
    m_min_aggr
        .update(&mut v, &DataValue::from(3))
        .expect("test assertion");
    assert_eq!(v, DataValue::from(1));
}

#[test]
fn test_max() {
    let mut aggr = parse_aggr("max").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");
    aggr.meet_init(&[]).expect("test assertion");

    let mut max_aggr = aggr.normal_op.expect("test assertion");
    max_aggr.set(&DataValue::from(10)).expect("test assertion");
    max_aggr.set(&DataValue::from(9)).expect("test assertion");
    max_aggr.set(&DataValue::from(1)).expect("test assertion");
    max_aggr.set(&DataValue::from(2)).expect("test assertion");
    max_aggr.set(&DataValue::from(3)).expect("test assertion");
    assert_eq!(max_aggr.get().expect("test assertion"), DataValue::from(10));

    let m_max_aggr = aggr.meet_op.expect("test assertion");
    let mut v = DataValue::from(5);
    m_max_aggr
        .update(&mut v, &DataValue::from(10))
        .expect("test assertion");
    m_max_aggr
        .update(&mut v, &DataValue::from(9))
        .expect("test assertion");
    m_max_aggr
        .update(&mut v, &DataValue::from(1))
        .expect("test assertion");
    m_max_aggr
        .update(&mut v, &DataValue::from(2))
        .expect("test assertion");
    m_max_aggr
        .update(&mut v, &DataValue::from(3))
        .expect("test assertion");
    assert_eq!(v, DataValue::from(10));
}

#[test]
fn test_choice_rand() {
    let mut aggr = parse_aggr("choice_rand").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");

    let mut choice_aggr = aggr.normal_op.expect("test assertion");
    choice_aggr
        .set(&DataValue::from(1))
        .expect("test assertion");
    choice_aggr
        .set(&DataValue::from(2))
        .expect("test assertion");
    choice_aggr
        .set(&DataValue::from(3))
        .expect("test assertion");
    let v = choice_aggr
        .get()
        .expect("test assertion")
        .get_int()
        .expect("test assertion");
    assert!(v == 1 || v == 2 || v == 3);
}

#[test]
fn test_min_cost() {
    let mut aggr = parse_aggr("min_cost").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");
    aggr.meet_init(&[]).expect("test assertion");

    let mut min_cost_aggr = aggr.normal_op.expect("test assertion");
    min_cost_aggr
        .set(&DataValue::List(vec![DataValue::Null, DataValue::from(3)]))
        .expect("test assertion");
    min_cost_aggr
        .set(&DataValue::List(vec![
            DataValue::from(true),
            DataValue::from(1),
        ]))
        .expect("test assertion");
    min_cost_aggr
        .set(&DataValue::List(vec![
            DataValue::from(false),
            DataValue::from(2),
        ]))
        .expect("test assertion");
    assert_eq!(
        min_cost_aggr.get().expect("test assertion"),
        DataValue::List(vec![DataValue::from(true), DataValue::from(1.)])
    );

    let m_min_cost_aggr = aggr.meet_op.expect("test assertion");
    let mut v = DataValue::List(vec![DataValue::Null, DataValue::from(3)]);
    m_min_cost_aggr
        .update(
            &mut v,
            &DataValue::List(vec![DataValue::from(true), DataValue::from(1)]),
        )
        .expect("test assertion");
    m_min_cost_aggr
        .update(
            &mut v,
            &DataValue::List(vec![DataValue::from(false), DataValue::from(2)]),
        )
        .expect("test assertion");
    assert_eq!(
        v,
        DataValue::List(vec![DataValue::from(true), DataValue::from(1)])
    );
}

#[test]
fn test_latest_by() {
    let mut aggr = parse_aggr("latest_by").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");

    let mut latest_by_aggr = aggr.normal_op.expect("test assertion");
    latest_by_aggr
        .set(&DataValue::List(vec![DataValue::Null, DataValue::from(3)]))
        .expect("test assertion");
    latest_by_aggr
        .set(&DataValue::List(vec![
            DataValue::from(true),
            DataValue::from(1),
        ]))
        .expect("test assertion");
    latest_by_aggr
        .set(&DataValue::List(vec![
            DataValue::from(false),
            DataValue::from(2),
        ]))
        .expect("test assertion");
    assert_eq!(
        latest_by_aggr.get().expect("test assertion"),
        DataValue::Null
    );
}

#[test]
fn test_shortest() {
    let mut aggr = parse_aggr("shortest").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");
    aggr.meet_init(&[]).expect("test assertion");

    let mut shortest_aggr = aggr.normal_op.expect("test assertion");
    shortest_aggr
        .set(&DataValue::List(
            [1, 2, 3].into_iter().map(DataValue::from).collect(),
        ))
        .expect("test assertion");
    shortest_aggr
        .set(&DataValue::List(
            [2].into_iter().map(DataValue::from).collect(),
        ))
        .expect("test assertion");
    shortest_aggr
        .set(&DataValue::List(
            [2, 3].into_iter().map(DataValue::from).collect(),
        ))
        .expect("test assertion");
    assert_eq!(
        shortest_aggr.get().expect("test assertion"),
        DataValue::List([2].into_iter().map(DataValue::from).collect())
    );

    let m_shortest_aggr = aggr.meet_op.expect("test assertion");
    let mut v = DataValue::List([1, 2, 3].into_iter().map(DataValue::from).collect());
    m_shortest_aggr
        .update(
            &mut v,
            &DataValue::List([2].into_iter().map(DataValue::from).collect()),
        )
        .expect("test assertion");
    m_shortest_aggr
        .update(
            &mut v,
            &DataValue::List([2, 3].into_iter().map(DataValue::from).collect()),
        )
        .expect("test assertion");
    assert_eq!(
        v,
        DataValue::List([2].into_iter().map(DataValue::from).collect())
    );
}

#[test]
fn test_choice() {
    let mut aggr = parse_aggr("choice").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");
    aggr.meet_init(&[]).expect("test assertion");

    let mut choice_aggr = aggr.normal_op.expect("test assertion");
    choice_aggr.set(&DataValue::Null).expect("test assertion");
    choice_aggr
        .set(&DataValue::from(1))
        .expect("test assertion");
    choice_aggr
        .set(&DataValue::from(2))
        .expect("test assertion");
    assert_eq!(
        choice_aggr.get().expect("test assertion"),
        DataValue::from(1)
    );

    let m_coalesce_aggr = aggr.meet_op.expect("test assertion");
    let mut v = DataValue::Null;
    m_coalesce_aggr
        .update(
            &mut v,
            &DataValue::List([2].into_iter().map(DataValue::from).collect()),
        )
        .expect("test assertion");
    m_coalesce_aggr
        .update(
            &mut v,
            &DataValue::List([2, 3].into_iter().map(DataValue::from).collect()),
        )
        .expect("test assertion");
    assert_eq!(
        v,
        DataValue::List([2].into_iter().map(DataValue::from).collect())
    );
}

#[test]
fn test_bit_and() {
    let mut aggr = parse_aggr("bit_and").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");
    aggr.meet_init(&[]).expect("test assertion");

    let mut bit_and_aggr = aggr.normal_op.expect("test assertion");
    bit_and_aggr
        .set(&DataValue::Bytes(vec![0b11100]))
        .expect("test assertion");
    bit_and_aggr
        .set(&DataValue::Bytes(vec![0b01011]))
        .expect("test assertion");
    assert_eq!(
        bit_and_aggr.get().expect("test assertion"),
        DataValue::Bytes(vec![0b01000])
    );

    let m_bit_and_aggr = aggr.meet_op.expect("test assertion");
    let mut v = DataValue::Bytes(vec![0b11100]);
    m_bit_and_aggr
        .update(&mut v, &DataValue::Bytes(vec![0b01011]))
        .expect("test assertion");
    assert_eq!(v, DataValue::Bytes(vec![0b01000]));
}

#[test]
fn test_bit_or() {
    let mut aggr = parse_aggr("bit_or").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");
    aggr.meet_init(&[]).expect("test assertion");

    let mut bit_or_aggr = aggr.normal_op.expect("test assertion");
    bit_or_aggr
        .set(&DataValue::Bytes(vec![0b11100]))
        .expect("test assertion");
    bit_or_aggr
        .set(&DataValue::Bytes(vec![0b01011]))
        .expect("test assertion");
    assert_eq!(
        bit_or_aggr.get().expect("test assertion"),
        DataValue::Bytes(vec![0b11111])
    );

    let m_bit_or_aggr = aggr.meet_op.expect("test assertion");
    let mut v = DataValue::Bytes(vec![0b11100]);
    m_bit_or_aggr
        .update(&mut v, &DataValue::Bytes(vec![0b01011]))
        .expect("test assertion");
    assert_eq!(v, DataValue::Bytes(vec![0b11111]));
}

#[test]
fn test_bit_xor() {
    let mut aggr = parse_aggr("bit_xor").expect("test assertion").clone();
    aggr.normal_init(&[]).expect("test assertion");

    let mut bit_xor_aggr = aggr.normal_op.expect("test assertion");
    bit_xor_aggr
        .set(&DataValue::Bytes(vec![0b11100]))
        .expect("test assertion");
    bit_xor_aggr
        .set(&DataValue::Bytes(vec![0b01011]))
        .expect("test assertion");
    assert_eq!(
        bit_xor_aggr.get().expect("test assertion"),
        DataValue::Bytes(vec![0b10111])
    );
}
