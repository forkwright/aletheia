//! Basic query tests: limits, aggregation, conditions, classical logic.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use serde_json::json;
use tracing::debug;

use crate::engine::DbInstance;
use crate::engine::data::value::DataValue;

#[test]
fn test_limit_offset() {
    let db = DbInstance::default();
    let res = db
        .run_default("?[a] := a in [5,3,1,2,4] :limit 2")
        .expect("limit query should succeed")
        .into_json();
    assert_eq!(
        res["rows"],
        json!([[3], [5]]),
        "limit 2 should return first 2 sorted rows"
    );
    let res = db
        .run_default("?[a] := a in [5,3,1,2,4] :limit 2 :offset 1")
        .expect("limit+offset query should succeed")
        .into_json();
    assert_eq!(
        res["rows"],
        json!([[1], [3]]),
        "limit 2 offset 1 should skip first row"
    );
    let res = db
        .run_default("?[a] := a in [5,3,1,2,4] :limit 2 :offset 4")
        .expect("limit+offset at end should succeed")
        .into_json();
    assert_eq!(
        res["rows"],
        json!([[4]]),
        "limit 2 offset 4 should return one remaining row"
    );
    let res = db
        .run_default("?[a] := a in [5,3,1,2,4] :limit 2 :offset 5")
        .expect("limit+offset past end should succeed")
        .into_json();
    assert_eq!(
        res["rows"],
        json!([]),
        "limit 2 offset 5 should return empty result"
    );
}

#[test]
fn test_normal_aggr_empty() {
    let db = DbInstance::default();
    let res = db
        .run_default("?[count(a)] := a in []")
        .expect("count over empty set should succeed")
        .rows;
    assert_eq!(
        res,
        vec![vec![DataValue::from(0)]],
        "count over empty set should return 0"
    );
}

#[test]
fn test_meet_aggr_empty() {
    let db = DbInstance::default();
    let res = db
        .run_default("?[min(a)] := a in []")
        .expect("min over empty set should succeed")
        .rows;
    assert_eq!(
        res,
        vec![vec![DataValue::Null]],
        "min over empty set should return Null"
    );

    let res = db
        .run_default("?[min(a), count(a)] := a in []")
        .expect("min and count over empty set should succeed")
        .rows;
    assert_eq!(
        res,
        vec![vec![DataValue::Null, DataValue::from(0)]],
        "min should be Null and count should be 0 for empty set"
    );
}

#[test]
fn test_layers() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    let db = DbInstance::default();
    let res = db
        .run_default(
            r#"
        y[a] := a in [1,2,3]
        x[sum(a)] := y[a]
        x[sum(a)] := a in [4,5,6]
        ?[sum(a)] := x[a]
        "#,
        )
        .expect("layered sum query should succeed")
        .rows;
    assert_eq!(res[0][0], DataValue::from(21.), "sum of 1..=6 should be 21")
}

#[test]
fn test_conditions() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let db = DbInstance::default();
    db.run_default(
        r#"
        {
            ?[code] <- [['a'],['b'],['c']]
            :create airport {code}
        }
        {
            ?[fr, to, dist] <- [['a', 'b', 1.1], ['a', 'c', 0.5], ['b', 'c', 9.1]]
            :create route {fr, to => dist}
        }
        "#,
    )
    .expect("test setup of airports and routes should succeed");
    debug!("real test begins");
    let res = db
        .run_default(
            r#"
        r[code, dist] := *airport{code}, *route{fr: code, dist};
        ?[dist] := r['a', dist], dist > 0.5, dist <= 1.1;
        "#,
        )
        .expect("filtered route query should succeed")
        .rows;
    assert_eq!(
        res[0][0],
        DataValue::from(1.1),
        "only route with dist 1.1 should pass the filter"
    )
}

#[test]
fn test_classical() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let db = DbInstance::default();
    let res = db
        .run_default(
            r#"
parent[] <- [['joseph', 'jakob'],
             ['jakob', 'isaac'],
             ['isaac', 'abraham']]
grandparent[gcld, gp] := parent[gcld, p], parent[p, gp]
?[who] := grandparent[who, 'abraham']
        "#,
        )
        .expect("grandparent query should succeed")
        .rows;
    assert_eq!(
        res[0][0],
        DataValue::from("jakob"),
        "jakob should be the grandchild of abraham"
    )
}

#[test]
fn default_columns() {
    let db = DbInstance::default();

    db.run_default(
        r#"
            :create status {uid: String, ts default now() => quitted: Bool, mood: String}
            "#,
    )
    .expect("creating status relation with default columns should succeed");

    db.run_default(
        r#"
        ?[uid, quitted, mood] <- [['z', true, 'x']]
            :put status {uid => quitted, mood}
        "#,
    )
    .expect("inserting row into status should succeed");
}

#[test]
fn rm_does_not_need_all_keys() {
    let db = DbInstance::default();
    db.run_default(":create status {uid => mood}")
        .expect("creating status relation should succeed");
    assert!(
        db.run_default("?[uid, mood] <- [[1, 2]] :put status {uid => mood}",)
            .is_ok(),
        "putting a fully-specified row should succeed"
    );
    assert!(
        db.run_default("?[uid, mood] <- [[2]] :put status {uid}",)
            .is_err(),
        "putting a row with missing value columns should fail"
    );
    assert!(
        db.run_default("?[uid, mood] <- [[3, 2]] :rm status {uid => mood}",)
            .is_ok(),
        "removing with all keys specified should succeed"
    );
    assert!(
        db.run_default("?[uid] <- [[1]] :rm status {uid}").is_ok(),
        "removing with only key columns should succeed"
    );
}

#[cfg(feature = "graph-algo")]
#[test]
fn strict_checks_for_fixed_rules_args() {
    let db = DbInstance::default();
    let res = db.run_default(
        r#"
            r[] <- [[1, 2]]
            ?[] <~ PageRank(r[_, _])
        "#,
    );
    assert!(res.is_ok(), "PageRank with wildcard binding should succeed");

    let db = DbInstance::default();
    let res = db.run_default(
        r#"
            r[] <- [[1, 2]]
            ?[] <~ PageRank(r[a, b])
        "#,
    );
    assert!(
        res.is_ok(),
        "PageRank with named distinct bindings should succeed"
    );

    let db = DbInstance::default();
    let res = db.run_default(
        r#"
            r[] <- [[1, 2]]
            ?[] <~ PageRank(r[a, a])
        "#,
    );
    assert!(
        res.is_err(),
        "PageRank with duplicate variable binding should fail"
    );
}

#[test]
fn do_not_unify_underscore() {
    let db = DbInstance::default();
    let res = db
        .run_default(
            r#"
        r1[] <- [[1, 'a'], [2, 'b']]
        r2[] <- [[2, 'B'], [3, 'C']]

        ?[l1, l2] := r1[_ , l1], r2[_ , l2]
        "#,
        )
        .expect("cross product with underscore binding should succeed")
        .rows;
    assert_eq!(
        res.len(),
        4,
        "cross product of 2x2 relations should produce 4 rows"
    );

    let res = db.run_default(
        r#"
        ?[_] := _ = 1
        "#,
    );
    assert!(res.is_err(), "binding underscore in query head should fail");

    let res = db
        .run_default(
            r#"
        ?[x] := x = 1, _ = 1, _ = 2
        "#,
        )
        .expect("using underscore in body (not head) should succeed")
        .rows;

    assert_eq!(
        res.len(),
        1,
        "query with underscore in body should return one row"
    );
}

#[test]
fn imperative_script() {}

#[test]
fn returning_relations() {
    let db = DbInstance::default();
    let res = db
        .run_default(
            r#"
        {:create _xxz {a}}
        {?[a] := a in [5,4,1,2,3] :put _xxz {a}}
        {?[a] := *_xxz[a], a % 2 == 0 :rm _xxz {a}}
        {?[a] := *_xxz[b], a = b * 2}
        "#,
        )
        .expect("imperative returning relations script should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([[2], [6], [10]]),
        "doubled odd values 1,3,5 should be 2,6,10"
    );
    let res = db.run_default(
        r#"
        {?[a] := *_xxz[b], a = b * 2}
        "#,
    );
    assert!(
        res.is_err(),
        "accessing temp relation _xxz outside its script should fail"
    );
}
