//! Integration tests for the engine runtime.
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]
use std::collections::BTreeMap;
use std::time::Duration;

use compact_str::CompactString;
use itertools::Itertools;
use serde_json::json;
use tracing::debug;

use crate::engine::DbInstance;
use crate::engine::data::expr::Expr;
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::DataValue;
use crate::engine::error::InternalResult as Result;
use crate::engine::fixed_rule::{FixedRule, FixedRulePayload};
use crate::engine::fts::{TokenizerCache, TokenizerConfig};
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::callback::CallbackOp;
use crate::engine::runtime::db::{Poison, ScriptMutability};
use crate::engine::runtime::temp_store::RegularTempStore;

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
    println!("{:?}", res);
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
    println!("{:?}", res);
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

#[test]
fn test_trigger() {
    let db = DbInstance::default();
    db.run_default(":create friends {fr: Int, to: Int => data: Any}")
        .expect("creating friends relation should succeed");
    db.run_default(":create friends.rev {to: Int, fr: Int => data: Any}")
        .expect("creating friends.rev relation should succeed");
    db.run_default(
        r#"
        ::set_triggers friends

        on put {
            ?[fr, to, data] := _new[fr, to, data]

            :put friends.rev{ to, fr => data}
        }
        on rm {
            ?[fr, to] := _old[fr, to, data]

            :rm friends.rev{ to, fr }
        }
        "#,
    )
    .expect("setting triggers on friends should succeed");
    db.run_default(r"?[fr, to, data] <- [[1,2,3]] :put friends {fr, to => data}")
        .expect("inserting into friends should succeed");
    let ret = db
        .export_relations(["friends", "friends.rev"].into_iter())
        .expect("exporting friends and friends.rev should succeed");
    let frs = ret
        .get("friends")
        .expect("friends relation should be present in export");
    assert_eq!(
        vec![DataValue::from(1), DataValue::from(2), DataValue::from(3)],
        frs.rows[0],
        "friends row should contain [1, 2, 3]"
    );

    let frs_rev = ret
        .get("friends.rev")
        .expect("friends.rev relation should be present in export");
    assert_eq!(
        vec![DataValue::from(2), DataValue::from(1), DataValue::from(3)],
        frs_rev.rows[0],
        "friends.rev trigger should reverse fr and to"
    );
    db.run_default(r"?[fr, to] <- [[1,2], [2,3]] :rm friends {fr, to}")
        .expect("removing from friends should succeed");
    let ret = db
        .export_relations(["friends", "friends.rev"].into_iter())
        .expect("re-exporting relations after removal should succeed");
    let frs = ret
        .get("friends")
        .expect("friends relation should still be present after removal");
    assert!(
        frs.rows.is_empty(),
        "friends should be empty after removing all rows"
    );
}

#[test]
fn test_callback() {
    let db = DbInstance::default();
    let mut collected = vec![];
    let (_id, receiver) = db.register_callback("friends", None);
    db.run_default(":create friends {fr: Int, to: Int => data: Any}")
        .expect("creating friends relation should succeed");
    db.run_default(r"?[fr, to, data] <- [[1,2,3],[4,5,6]] :put friends {fr, to => data}")
        .expect("initial put into friends should succeed");
    db.run_default(r"?[fr, to, data] <- [[1,2,4],[4,7,6]] :put friends {fr, to => data}")
        .expect("second put into friends should succeed");
    db.run_default(r"?[fr, to] <- [[1,9],[4,5]] :rm friends {fr, to}")
        .expect("removing from friends should succeed");
    std::thread::sleep(Duration::from_secs_f64(0.01));
    while let Ok(d) = receiver.try_recv() {
        collected.push(d);
    }
    let collected = collected;
    assert_eq!(
        collected[0].0,
        CallbackOp::Put,
        "first callback should be a Put operation"
    );
    assert_eq!(
        collected[0].1.rows.len(),
        2,
        "first put should have 2 new rows"
    );
    assert_eq!(
        collected[0].1.rows[0].len(),
        3,
        "first put new rows should have 3 columns"
    );
    assert_eq!(
        collected[0].2.rows.len(),
        0,
        "first put should have no old rows"
    );
    assert_eq!(
        collected[1].0,
        CallbackOp::Put,
        "second callback should be a Put operation"
    );
    assert_eq!(
        collected[1].1.rows.len(),
        2,
        "second put should have 2 new rows"
    );
    assert_eq!(
        collected[1].1.rows[0].len(),
        3,
        "second put new rows should have 3 columns"
    );
    assert_eq!(
        collected[1].2.rows.len(),
        1,
        "second put should have 1 replaced old row"
    );
    assert_eq!(
        collected[1].2.rows[0],
        vec![DataValue::from(1), DataValue::from(2), DataValue::from(3)],
        "replaced row should be [1, 2, 3]"
    );
    assert_eq!(
        collected[2].0,
        CallbackOp::Rm,
        "third callback should be a Rm operation"
    );
    assert_eq!(
        collected[2].1.rows.len(),
        2,
        "rm should report 2 requested rows"
    );
    assert_eq!(
        collected[2].1.rows[0].len(),
        2,
        "rm requested rows should have 2 columns (key only)"
    );
    assert_eq!(
        collected[2].2.rows.len(),
        1,
        "rm should have 1 actually deleted row"
    );
    assert_eq!(
        collected[2].2.rows[0].len(),
        3,
        "rm deleted row should have 3 columns"
    );
}

#[test]
fn test_update() {
    let db = DbInstance::default();
    db.run_default(":create friends {fr: Int, to: Int => a: Any, b: Any, c: Any}")
        .expect("creating friends relation should succeed");
    db.run_default("?[fr, to, a, b, c] <- [[1,2,3,4,5]] :put friends {fr, to => a, b, c}")
        .expect("inserting initial row into friends should succeed");
    let res = db
        .run_default("?[fr, to, a, b, c] := *friends{fr, to, a, b, c}")
        .expect("querying friends should succeed")
        .into_json();
    assert_eq!(
        res["rows"][0],
        json!([1, 2, 3, 4, 5]),
        "initial row should be [1, 2, 3, 4, 5]"
    );
    db.run_default("?[fr, to, b] <- [[1, 2, 100]] :update friends {fr, to => b}")
        .expect("partial update of friends should succeed");
    let res = db
        .run_default("?[fr, to, a, b, c] := *friends{fr, to, a, b, c}")
        .expect("querying friends after update should succeed")
        .into_json();
    assert_eq!(
        res["rows"][0],
        json!([1, 2, 3, 100, 5]),
        "after updating b to 100, row should be [1, 2, 3, 100, 5]"
    );
}

#[test]
fn test_index() {
    let db = DbInstance::default();
    db.run_default(":create friends {fr: Int, to: Int => data: Any}")
        .expect("creating friends relation should succeed");

    db.run_default(r"?[fr, to, data] <- [[1,2,3],[4,5,6]] :put friends {fr, to, data}")
        .expect("inserting initial rows into friends should succeed");

    assert!(
        db.run_default("::index create friends:rev {to, no}")
            .is_err(),
        "creating index with non-existent column should fail"
    );
    db.run_default("::index create friends:rev {to, data}")
        .expect("creating index on valid columns should succeed");

    db.run_default(r"?[fr, to, data] <- [[1,2,5],[6,5,7]] :put friends {fr, to => data}")
        .expect("updating rows in friends should succeed");
    db.run_default(r"?[fr, to] <- [[4,5]] :rm friends {fr, to}")
        .expect("removing row from friends should succeed");

    let rels_data = db
        .export_relations(["friends", "friends:rev"].into_iter())
        .expect("exporting friends and index should succeed");
    assert_eq!(
        rels_data["friends"].clone().into_json()["rows"],
        json!([[1, 2, 5], [6, 5, 7]]),
        "friends should contain updated rows"
    );
    assert_eq!(
        rels_data["friends:rev"].clone().into_json()["rows"],
        json!([[2, 5, 1], [5, 7, 6]]),
        "friends:rev index should reflect updated rows"
    );

    let rels = db
        .run_default("::relations")
        .expect("listing relations should succeed");
    assert_eq!(
        rels.rows[1][0],
        DataValue::from("friends:rev"),
        "second relation should be friends:rev"
    );
    assert_eq!(
        rels.rows[1][1],
        DataValue::from(3),
        "friends:rev should have 3 columns"
    );
    assert_eq!(
        rels.rows[1][2],
        DataValue::from("index"),
        "friends:rev should have type 'index'"
    );

    let cols = db
        .run_default("::columns friends:rev")
        .expect("listing columns of friends:rev should succeed");
    assert_eq!(
        cols.rows.len(),
        3,
        "friends:rev index should have 3 columns"
    );

    let res = db
        .run_default("?[fr, data] := *friends:rev{to: 2, fr, data}")
        .expect("querying index directly should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([[1, 5]]),
        "index lookup by to=2 should return fr=1, data=5"
    );

    let res = db
        .run_default("?[fr, data] := *friends{to: 2, fr, data}")
        .expect("querying friends by non-key column should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([[1, 5]]),
        "reverse lookup via index should return fr=1, data=5"
    );

    let expl = db
        .run_default("::explain { ?[fr, data] := *friends{to: 2, fr, data} }")
        .expect("explain query should succeed");
    let joins = expl.into_json()["rows"]
        .as_array()
        .expect("explain rows should be a JSON array")
        .iter()
        .map(|row| {
            row.as_array()
                .expect("each explain row should be a JSON array")[5]
                .clone()
        })
        .collect_vec();
    assert!(
        joins.contains(&json!(":friends:rev")),
        "query plan should use the friends:rev index"
    );
    db.run_default("::index drop friends:rev")
        .expect("dropping friends:rev index should succeed");
}

#[test]
fn test_json_objects() {
    let db = DbInstance::default();
    db.run_default("?[a] := a = {'a': 1}")
        .expect("inline JSON object query should succeed");
    db.run_default(
        r"?[a] := a = {
            'a': 1
        }",
    )
    .expect("multiline JSON object query should succeed");
}

#[test]
fn test_custom_rules() {
    let db = DbInstance::default();
    struct Custom;

    impl FixedRule for Custom {
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
            payload: FixedRulePayload<'_, '_>,
            out: &'_ mut RegularTempStore,
            _poison: Poison,
        ) -> Result<()> {
            let rel = payload.get_input(0)?;
            let mult = payload.integer_option("mult", Some(2))?;
            for maybe_row in rel.iter()? {
                let row = maybe_row?;
                let mut sum = 0;
                for col in row {
                    let d = col.get_int().unwrap_or(0);
                    sum += d;
                }
                sum *= mult;
                out.put(vec![DataValue::from(sum)])
            }
            Ok(())
        }
    }

    db.register_fixed_rule("SumCols".to_string(), Custom)
        .expect("registering custom SumCols rule should succeed");
    let res = db
        .run_default(
            r#"
        rel[] <- [[1,2,3,4],[5,6,7,8]]
        ?[x] <~ SumCols(rel[], mult: 100)
    "#,
        )
        .expect("running custom SumCols rule should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([[1000], [2600]]),
        "SumCols with mult=100 should produce 1000 and 2600"
    );
}

#[test]
fn test_index_short() {
    let db = DbInstance::default();
    db.run_default(":create friends {fr: Int, to: Int => data: Any}")
        .expect("creating friends relation should succeed");

    db.run_default(r"?[fr, to, data] <- [[1,2,3],[4,5,6]] :put friends {fr, to => data}")
        .expect("inserting initial rows into friends should succeed");

    db.run_default("::index create friends:rev {to}")
        .expect("creating short index on 'to' should succeed");

    db.run_default(r"?[fr, to, data] <- [[1,2,5],[6,5,7]] :put friends {fr, to => data}")
        .expect("updating rows in friends should succeed");
    db.run_default(r"?[fr, to] <- [[4,5]] :rm friends {fr, to}")
        .expect("removing row from friends should succeed");

    let rels_data = db
        .export_relations(["friends", "friends:rev"].into_iter())
        .expect("exporting friends and short index should succeed");
    assert_eq!(
        rels_data["friends"].clone().into_json()["rows"],
        json!([[1, 2, 5], [6, 5, 7]]),
        "friends should contain updated rows"
    );
    assert_eq!(
        rels_data["friends:rev"].clone().into_json()["rows"],
        json!([[2, 1], [5, 6]]),
        "short index should contain to+fr key pairs only"
    );

    let rels = db
        .run_default("::relations")
        .expect("listing relations should succeed");
    assert_eq!(
        rels.rows[1][0],
        DataValue::from("friends:rev"),
        "second relation should be friends:rev"
    );
    assert_eq!(
        rels.rows[1][1],
        DataValue::from(2),
        "short friends:rev should have 2 columns"
    );
    assert_eq!(
        rels.rows[1][2],
        DataValue::from("index"),
        "friends:rev should have type 'index'"
    );

    let cols = db
        .run_default("::columns friends:rev")
        .expect("listing columns of short index should succeed");
    assert_eq!(
        cols.rows.len(),
        2,
        "short friends:rev index should have 2 columns"
    );

    let expl = db
        .run_default("::explain { ?[fr, data] := *friends{to: 2, fr, data} }")
        .expect("explain query should succeed")
        .into_json();

    for row in expl["rows"]
        .as_array()
        .expect("explain rows should be a JSON array")
    {
        println!("{}", row);
    }

    let joins = expl["rows"]
        .as_array()
        .expect("explain rows should be a JSON array")
        .iter()
        .map(|row| {
            row.as_array()
                .expect("each explain row should be a JSON array")[5]
                .clone()
        })
        .collect_vec();
    assert!(
        joins.contains(&json!(":friends:rev")),
        "query plan should use the friends:rev index"
    );

    let res = db
        .run_default("?[fr, data] := *friends{to: 2, fr, data}")
        .expect("querying friends by non-key column should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([[1, 5]]),
        "reverse lookup should return fr=1, data=5"
    );
}

#[test]
fn test_multi_tx() {
    let db = DbInstance::default();
    let tx = db.multi_transaction_test(true);
    tx.run_script(":create a {a}", Default::default())
        .expect("creating relation in tx should succeed");
    tx.run_script("?[a] <- [[1]] :put a {a}", Default::default())
        .expect("inserting row 1 in tx should succeed");
    assert!(
        tx.run_script(":create a {a}", Default::default()).is_err(),
        "re-creating existing relation in tx should fail"
    );
    tx.run_script("?[a] <- [[2]] :put a {a}", Default::default())
        .expect("inserting row 2 in tx should succeed");
    tx.run_script("?[a] <- [[3]] :put a {a}", Default::default())
        .expect("inserting row 3 in tx should succeed");
    tx.commit().expect("committing transaction should succeed");
    assert_eq!(
        db.run_default("?[a] := *a[a]")
            .expect("querying after commit should succeed")
            .into_json()["rows"],
        json!([[1], [2], [3]]),
        "committed transaction should persist all 3 rows"
    );

    let db = DbInstance::default();
    let tx = db.multi_transaction_test(true);
    tx.run_script(":create a {a}", Default::default())
        .expect("creating relation in tx should succeed");
    tx.run_script("?[a] <- [[1]] :put a {a}", Default::default())
        .expect("inserting row 1 in tx should succeed");
    assert!(
        tx.run_script(":create a {a}", Default::default()).is_err(),
        "re-creating existing relation in tx should fail"
    );
    tx.run_script("?[a] <- [[2]] :put a {a}", Default::default())
        .expect("inserting row 2 in tx should succeed");
    tx.run_script("?[a] <- [[3]] :put a {a}", Default::default())
        .expect("inserting row 3 in tx should succeed");
    tx.abort().expect("aborting transaction should succeed");
    assert!(
        db.run_default("?[a] := *a[a]").is_err(),
        "query after aborted tx should fail as relation was not committed"
    );
}

#[test]
fn test_vec_types() {
    let db = DbInstance::default();
    db.run_default(":create a {k: String => v: <F32; 8>}")
        .expect("creating relation with F32 vector column should succeed");
    db.run_default("?[k, v] <- [['k', [1,2,3,4,5,6,7,8]]] :put a {k => v}")
        .expect("inserting row with vector value should succeed");
    let res = db
        .run_default("?[k, v] := *a{k, v}")
        .expect("querying vector relation should succeed");
    assert_eq!(
        json!([1., 2., 3., 4., 5., 6., 7., 8.]),
        res.into_json()["rows"][0][1],
        "stored vector should round-trip as floats"
    );
    let res = db
        .run_default("?[v] <- [[vec([1,2,3,4,5,6,7,8])]]")
        .expect("vec() constructor query should succeed");
    assert_eq!(
        json!([1., 2., 3., 4., 5., 6., 7., 8.]),
        res.into_json()["rows"][0][0],
        "vec() constructor should produce expected float array"
    );
    let res = db
        .run_default("?[v] <- [[rand_vec(5)]]")
        .expect("rand_vec query should succeed");
    assert_eq!(
        5,
        res.into_json()["rows"][0][0]
            .as_array()
            .expect("rand_vec result should be a JSON array")
            .len(),
        "rand_vec(5) should produce a vector of length 5"
    );
    let res = db
        .run_default(r#"
            val[v] <- [[vec([1,2,3,4,5,6,7,8])]]
            ?[x,y,z] := val[v], x=l2_dist(v, v), y=cos_dist(v, v), nv = l2_normalize(v), z=ip_dist(nv, nv)
        "#)
        .expect("vector distance and normalize query should succeed");
    println!("{}", res.into_json());
}

#[test]
fn test_vec_index_insertion() {
    let db = DbInstance::default();
    db.run_default(
        r"
        ?[k, v, m] <- [['a', [1,2], true],
                       ['b', [2,3], false]]

        :create a {k: String => v: <F32; 2>, m: Bool}
    ",
    )
    .expect("creating vector relation with filter should succeed");
    db.run_default(
        r"
        ::hnsw create a:vec {
            dim: 2,
            m: 50,
            dtype: F32,
            fields: [v],
            distance: L2,
            ef_construction: 20,
            filter: m,
            #extend_candidates: true,
            #keep_pruned_connections: true,
        }",
    )
    .expect("creating HNSW index with filter should succeed");
    let res = db
        .run_default("?[k] := *a:vec{layer: 0, fr_k, to_k}, k = fr_k or k = to_k")
        .expect("querying HNSW index should succeed");
    assert_eq!(
        res.rows.len(),
        1,
        "only 'a' passes the filter m=true so only 1 node should be indexed"
    );
    println!("update!");
    db.run_default(r#"?[k, m] <- [["a", false]] :update a {}"#)
        .expect("updating a to m=false should succeed");
    let res = db
        .run_default("?[k] := *a:vec{layer: 0, fr_k, to_k}, k = fr_k or k = to_k")
        .expect("querying HNSW index after filter-disqualifying update should succeed");
    assert_eq!(
        res.rows.len(),
        0,
        "after updating a to m=false it should be removed from the index"
    );
    println!("{}", res.into_json());
}

#[test]
fn test_vec_index() {
    let db = DbInstance::default();
    db.run_default(
        r"
        ?[k, v] <- [['a', [1,2]],
                    ['b', [2,3]],
                    ['bb', [2,3]],
                    ['c', [3,4]],
                    ['x', [0,0.1]],
                    ['a', [112,0]],
                    ['b', [1,1]]]

        :create a {k: String => v: <F32; 2>}
    ",
    )
    .expect("creating vector relation with initial rows should succeed");
    db.run_default(
        r"
        ::hnsw create a:vec {
            dim: 2,
            m: 50,
            dtype: F32,
            fields: [v],
            distance: L2,
            ef_construction: 20,
            filter: k != 'k1',
            #extend_candidates: true,
            #keep_pruned_connections: true,
        }",
    )
    .expect("creating HNSW index with string filter should succeed");
    db.run_default(
        r"
        ?[k, v] <- [
                    ['a2', [1,25]],
                    ['b2', [2,34]],
                    ['bb2', [2,33]],
                    ['c2', [2,32]],
                    ['a2', [2,31]],
                    ['b2', [1,10]]
                    ]
        :put a {k => v}
        ",
    )
    .expect("inserting additional rows into vector relation should succeed");

    println!("all links");
    for (_, nrows) in db
        .export_relations(["a:vec"].iter())
        .expect("exporting HNSW index should succeed")
    {
        let nrows = nrows.rows;
        for row in nrows {
            println!("{} {} -> {} {}", row[0], row[1], row[4], row[7]);
        }
    }

    let res = db
        .run_default(
            r"
        #::explain {
        ?[dist, k, v] := ~a:vec{k, v | query: q, k: 2, ef: 20, bind_distance: dist}, q = vec([200, 34])
        #}
        ",
        )
        .expect("HNSW KNN query should succeed");
    println!("results");
    for row in res.into_json()["rows"]
        .as_array()
        .expect("KNN result rows should be a JSON array")
    {
        println!("{} {} {}", row[0], row[1], row[2]);
    }
}

#[test]
fn test_fts_indexing() {
    let db = DbInstance::default();
    db.run_default(r":create a {k: String => v: String}")
        .expect("creating FTS base relation should succeed");
    db.run_default(
        r"?[k, v] <- [['a', 'hello world!'], ['b', 'the world is round']] :put a {k => v}",
    )
    .expect("inserting initial FTS rows should succeed");
    db.run_default(
        r"::fts create a:fts {
            extractor: v,
            tokenizer: Simple,
            filters: [Lowercase, Stemmer('English'), Stopwords('en')]
        }",
    )
    .expect("creating FTS index should succeed");
    db.run_default(
        r"?[k, v] <- [
            ['b', 'the world is square!'],
            ['c', 'see you at the end of the world!'],
            ['d', 'the world is the world and makes the world go around']
        ] :put a {k => v}",
    )
    .expect("inserting additional rows for FTS indexing should succeed");
    let res = db
        .run_default(
            r"
        ?[word, src_k, offset_from, offset_to, position, total_length] :=
            *a:fts{word, src_k, offset_from, offset_to, position, total_length}
        ",
        )
        .expect("querying FTS index directly should succeed");
    for row in res.into_json()["rows"]
        .as_array()
        .expect("FTS index rows should be a JSON array")
    {
        println!("{}", row);
    }
    println!("query");
    let res = db
        .run_default(r"?[k, v, s] := ~a:fts{k, v | query: 'world', k: 2, bind_score: s}")
        .expect("FTS search query should succeed");
    for row in res.into_json()["rows"]
        .as_array()
        .expect("FTS search results should be a JSON array")
    {
        println!("{}", row);
    }
}

#[test]
fn test_lsh_indexing2() {
    for i in 1..10 {
        let f = i as f64 / 10.;
        let db = DbInstance::default();
        db.run_default(r":create a {k: String => v: String}")
            .expect("creating LSH base relation should succeed");
        db.run_script(
            r"::lsh create a:lsh {extractor: v, tokenizer: NGram, n_gram: 3, target_threshold: $t }",
            BTreeMap::from([("t".into(), f.into())]),
            ScriptMutability::Mutable
        )
            .expect("creating LSH index should succeed");
        db.run_default("?[k, v] <- [['a', 'ewiygfspeoighjsfcfxzdfncalsdf']] :put a {k => v}")
            .expect("inserting LSH row should succeed");
        let res = db
            .run_default("?[k] := ~a:lsh{k | query: 'ewiygfspeoighjsfcfxzdfncalsdf', k: 1}")
            .expect("LSH similarity search should succeed");
        assert!(
            !res.rows.is_empty(),
            "exact-match LSH query should return at least one result"
        );
    }
}

#[test]
fn test_lsh_indexing3() {
    for i in 1..10 {
        let f = i as f64 / 10.;
        let db = DbInstance::default();
        db.run_default(r":create text {id: String,  => text: String, url: String? default null, dt: Float default now(), dup_for: String? default null }")
            .expect("creating text relation should succeed");
        db.run_script(
            r"::lsh create text:lsh {
                    extractor: text,
                    # extract_filter: is_null(dup_for),
                    tokenizer: NGram,
                    n_perm: 200,
                    target_threshold: $t,
                    n_gram: 7,
                }",
            BTreeMap::from([("t".into(), f.into())]),
            ScriptMutability::Mutable,
        )
        .expect("creating LSH index on text should succeed");
        db.run_default(
            "?[id, text] <- [['a', 'This function first generates 32 random bytes using the os.urandom function. It then base64 encodes these bytes using base64.urlsafe_b64encode, removes the padding, and decodes the result to a string.']] :put text {id, text}",
        )
        .expect("inserting text row should succeed");
        let res = db
            .run_default(
                r#"?[id, dup_for] :=
    ~text:lsh{id: id, dup_for: dup_for, | query: "This function first generates 32 random bytes using the os.urandom function. It then base64 encodes these bytes using base64.urlsafe_b64encode, removes the padding, and decodes the result to a string.", }"#,
            )
            .expect("LSH similarity search on text should succeed");
        assert!(
            !res.rows.is_empty(),
            "exact-match LSH query should return at least one result"
        );
        println!("{}", res.into_json());
    }
}

#[test]
fn filtering() {
    let db = DbInstance::default();
    let res = db
        .run_default(
            r"
        {
            ?[x, y] <- [[1, 2]]
            :create _rel {x => y}
            :returning
        }
        {
            ?[x, y] := x = 1, *_rel{x, y: 3}, y = 2
        }
    ",
        )
        .expect("filter script should succeed");
    assert_eq!(
        0,
        res.rows.len(),
        "conflicting key constraint should yield 0 rows"
    );

    let res = db
        .run_default(
            r"
        {
            ?[x, u, y] <- [[1, 0, 2]]
            :create _rel {x, u => y}
            :returning
        }
        {
            ?[x, y] := x = 1, *_rel{x, y: 3}, y = 2
        }
    ",
        )
        .expect("filter script with compound key should succeed");
    assert_eq!(0, res.rows.len(), "compound key filter should yield 0 rows");
}

#[test]
fn test_lsh_indexing4() {
    for i in 1..10 {
        let f = i as f64 / 10.;
        let db = DbInstance::default();
        db.run_default(r":create a {k: String => v: String}")
            .expect("creating LSH base relation should succeed");
        db.run_script(
            r"::lsh create a:lsh {extractor: v, tokenizer: NGram, n_gram: 3, target_threshold: $t }",
            BTreeMap::from([("t".into(), f.into())]),
            ScriptMutability::Mutable
        )
            .expect("creating LSH index should succeed");
        db.run_default("?[k, v] <- [['a', 'ewiygfspeoighjsfcfxzdfncalsdf']] :put a {k => v}")
            .expect("inserting LSH row should succeed");
        db.run_default("?[k] <- [['a']] :rm a {k}")
            .expect("removing LSH row should succeed");
        let res = db
            .run_default("?[k] := ~a:lsh{k | query: 'ewiygfspeoighjsfcfxzdfncalsdf', k: 1}")
            .expect("LSH search after deletion should succeed");
        assert!(
            res.rows.is_empty(),
            "LSH search after deleting the only row should return empty"
        );
    }
}

#[test]
fn test_lsh_indexing() {
    let db = DbInstance::default();
    db.run_default(r":create a {k: String => v: String}")
        .expect("creating LSH base relation should succeed");
    db.run_default(
        r"?[k, v] <- [['a', 'hello world!'], ['b', 'the world is round']] :put a {k => v}",
    )
    .expect("inserting initial LSH rows should succeed");
    db.run_default(
        r"::lsh create a:lsh {extractor: v, tokenizer: Simple, n_gram: 3, target_threshold: 0.3 }",
    )
    .expect("creating LSH index should succeed");
    db.run_default(
        r"?[k, v] <- [
            ['b', 'the world is square!'],
            ['c', 'see you at the end of the world!'],
            ['d', 'the world is the world and makes the world go around'],
            ['e', 'the world is the world and makes the world not go around']
        ] :put a {k => v}",
    )
    .expect("inserting additional LSH rows should succeed");
    let res = db
        .run_default("::columns a:lsh")
        .expect("listing LSH index columns should succeed");
    for row in res.into_json()["rows"]
        .as_array()
        .expect("LSH columns result should be a JSON array")
    {
        println!("{}", row);
    }
    let _res = db
        .run_default(
            r"
        ?[src_k, hash] :=
            *a:lsh{src_k, hash}
        ",
        )
        .expect("querying LSH index directly should succeed");
    let _res = db
        .run_default(
            r"
        ?[k, minhash] :=
            *a:lsh:inv{k, minhash}
        ",
        )
        .expect("querying LSH inverse index should succeed");
    let res = db
        .run_default(
            r"
            ?[k, v] := ~a:lsh{k, v |
                query: 'see him at the end of the world',
            }
            ",
        )
        .expect("LSH similarity search should succeed");
    for row in res.into_json()["rows"]
        .as_array()
        .expect("LSH search results should be a JSON array")
    {
        println!("{}", row);
    }
    let res = db
        .run_default("::indices a")
        .expect("listing indices should succeed");
    for row in res.into_json()["rows"]
        .as_array()
        .expect("indices result should be a JSON array")
    {
        println!("{}", row);
    }
    db.run_default(r"::lsh drop a:lsh")
        .expect("dropping LSH index should succeed");
}

#[test]
fn test_insertions() {
    let db = DbInstance::default();
    db.run_default(r":create a {k => v: <F32; 1536> default rand_vec(1536)}")
        .expect("creating relation with default rand_vec column should succeed");
    db.run_default(r"?[k] <- [[1]] :put a {k}")
        .expect("inserting row with default vector should succeed");
    db.run_default(r"?[k, v] := *a{k, v}")
        .expect("querying relation with vector should succeed");
    db.run_default(
        r"::hnsw create a:i {
            fields: [v], dim: 1536, ef: 16, filter: k % 3 == 0,
            m: 32
        }",
    )
    .expect("creating HNSW index with numeric filter should succeed");
    db.run_default(r"?[count(fr_k)] := *a:i{fr_k}")
        .expect("counting HNSW index entries should succeed");
    db.run_default(r"?[k] <- [[1]] :put a {k}")
        .expect("reinserting row should succeed");
    db.run_default(r"?[k] := k in int_range(300) :put a {k}")
        .expect("bulk inserting 300 rows should succeed");
    let res = db
        .run_default(
            r"?[dist, k] := ~a:i{k | query: v, bind_distance: dist, k:10, ef: 50, filter: k % 2 == 0, radius: 245}, *a{k: 96, v}",
        )
        .expect("HNSW KNN query with filter and radius should succeed");
    println!("results");
    for row in res.into_json()["rows"]
        .as_array()
        .expect("KNN results should be a JSON array")
    {
        println!("{} {}", row[0], row[1]);
    }
}

#[test]
fn tokenizers() {
    let tokenizers = TokenizerCache::default();
    let tokenizer = tokenizers
        .get(
            "simple",
            &TokenizerConfig {
                name: "Simple".into(),
                args: vec![],
            },
            &[],
        )
        .expect("getting simple tokenizer from cache should succeed");

    let mut token_stream = tokenizer.token_stream("It is closer to Apache Lucene than to Elasticsearch or Apache Solr in the sense it is not an off-the-shelf search engine server, but rather a crate that can be used to build such a search engine.");
    while let Some(token) = token_stream.next() {
        println!("Token {:?}", token.text);
    }
}

#[test]
fn multi_index_vec() {
    let db = DbInstance::default();
    db.run_default(
        r#"
        :create product {
            id
            =>
            name,
            description,
            price,
            name_vec: <F32; 1>,
            description_vec: <F32; 1>
        }
        "#,
    )
    .expect("creating product relation with multiple vector columns should succeed");
    db.run_default(
        r#"
        ::hnsw create product:semantic{
            fields: [name_vec, description_vec],
            dim: 1,
            ef: 16,
            m: 32,
        }
        "#,
    )
    .expect("creating HNSW index over multiple vector fields should succeed");
    db.run_default(
        r#"
        ?[id, name, description, price, name_vec, description_vec] <- [[1, "name", "description", 100, [1], [1]]]

        :put product {id => name, description, price, name_vec, description_vec}
        "#,
    ).expect("inserting product row should succeed");
    let res = db
        .run_default("::indices product")
        .expect("listing product indices should succeed");
    for row in res.into_json()["rows"]
        .as_array()
        .expect("indices result should be a JSON array")
    {
        println!("{}", row);
    }
}

#[test]
fn ensure_not() {
    let db = DbInstance::default();
    db.run_default(
        r"
    %ignore_error { :create id_alloc{id: Int => next_id: Int, last_id: Int}}
%ignore_error {
    ?[id, next_id, last_id] <- [[0, 1, 1000]];
    :ensure_not id_alloc{id => next_id, last_id}
}
    ",
    )
    .expect("ensure_not idempotent script should succeed");
}

#[test]
fn insertion() {
    let db = DbInstance::default();
    db.run_default(r":create a {x => y}")
        .expect("creating relation should succeed");
    assert!(
        db.run_default(r"?[x, y] <- [[1, 2]] :insert a {x => y}",)
            .is_ok(),
        "first insert should succeed"
    );
    assert!(
        db.run_default(r"?[x, y] <- [[1, 3]] :insert a {x => y}",)
            .is_err(),
        "duplicate key insert should fail"
    );
}

#[test]
fn deletion() {
    let db = DbInstance::default();
    db.run_default(r":create a {x => y}")
        .expect("creating relation should succeed");
    assert!(
        db.run_default(r"?[x] <- [[1]] :delete a {x}").is_err(),
        "deleting non-existent row should fail"
    );
    assert!(
        db.run_default(r"?[x, y] <- [[1, 2]] :insert a {x => y}",)
            .is_ok(),
        "inserting row should succeed"
    );
    db.run_default(r"?[x] <- [[1]] :delete a {x}")
        .expect("deleting existing row should succeed");
}

#[test]
fn into_payload() {
    let db = DbInstance::default();
    db.run_default(r":create a {x => y}")
        .expect("creating relation a should succeed");
    db.run_default(r"?[x, y] <- [[1, 2], [3, 4]] :insert a {x => y}")
        .expect("inserting 2 rows should succeed");

    let mut res = db
        .run_default(r"?[x, y] := *a[x, y]")
        .expect("querying all rows should succeed");
    assert_eq!(res.rows.len(), 2, "query should return both inserted rows");

    let delete = res.clone().into_payload("a", "rm");
    db.run_script(delete.0.as_str(), delete.1, ScriptMutability::Mutable)
        .expect("running delete payload should succeed");
    assert_eq!(
        db.run_default(r"?[x, y] := *a[x, y]")
            .expect("querying after delete should succeed")
            .rows
            .len(),
        0,
        "all rows should be deleted"
    );

    db.run_default(r":create b {m => n}")
        .expect("creating relation b should succeed");
    res.headers = vec!["m".into(), "n".into()];
    let put = res.into_payload("b", "put");
    db.run_script(put.0.as_str(), put.1, ScriptMutability::Mutable)
        .expect("running put payload should succeed");
    assert_eq!(
        db.run_default(r"?[m, n] := *b[m, n]")
            .expect("querying relation b should succeed")
            .rows
            .len(),
        2,
        "both rows should be present in b after put"
    );
}

#[test]
fn returning() {
    let db = DbInstance::default();
    db.run_default(":create a {x => y}")
        .expect("creating relation should succeed");
    let res = db
        .run_default(r"?[x, y] <- [[1, 2]] :insert a {x => y} ")
        .expect("insert should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([["OK"]]),
        "insert without :returning should return OK"
    );

    let res = db
        .run_default(r"?[x, y] <- [[1, 3], [2, 4]] :returning :put a {x => y} ")
        .expect("put with :returning should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([["inserted", 1, 3], ["inserted", 2, 4], ["replaced", 1, 2]]),
        ":returning should show inserted and replaced rows"
    );

    let res = db
        .run_default(r"?[x] <- [[1], [4]] :returning :rm a {x} ")
        .expect("rm with :returning should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([
            ["requested", 1, null],
            ["requested", 4, null],
            ["deleted", 1, 3]
        ]),
        ":returning on rm should show requested and actually deleted rows"
    );
    db.run_default(r":create todo{id:Uuid default rand_uuid_v1() => label: String, done: Bool}")
        .expect("creating todo relation with UUID default should succeed");
    let res = db
        .run_default(r"?[label,done] <- [['milk',false]] :put todo{label,done} :returning")
        .expect("put into todo with :returning should succeed");
    assert_eq!(
        res.rows[0].len(),
        4,
        "todo returning row should have 4 columns including generated id"
    );
    for title in res.headers.iter() {
        print!("{} ", title);
    }
    println!();
    for row in res.into_json()["rows"]
        .as_array()
        .expect("returning rows should be a JSON array")
    {
        println!("{}", row);
    }
}

#[test]
fn parser_corner_case() {
    let db = DbInstance::default();
    db.run_default(r#"?[x] := x = 1 or x = 2"#)
        .expect("'or' keyword query should parse correctly");
    db.run_default(r#"?[C] := C = 1  orx[C] := C = 1"#)
        .expect("'orx' relation name adjacent to 'or' should parse correctly");
    db.run_default(r#"?[C] := C = true, C  inx[C] := C = 1"#)
        .expect("'inx' relation name adjacent to 'in' should parse correctly");
    db.run_default(r#"?[k] := k in int_range(300)"#)
        .expect("'in' with int_range should parse correctly");
    db.run_default(r#"ywcc[a] <- [[1]] noto[A] := ywcc[A] ?[A] := noto[A]"#)
        .expect("'noto' relation name adjacent to 'not' should parse correctly");
}

#[test]
fn as_store_in_imperative_script() {
    let db = DbInstance::default();
    let res = db
        .run_default(
            r#"
    { ?[x, y, z] <- [[1, 2, 3], [4, 5, 6]] } as _store
    { ?[x, y, z] := *_store{x, y, z} }
    "#,
        )
        .expect("as-store in imperative script should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([[1, 2, 3], [4, 5, 6]]),
        "stored result should contain both rows"
    );
    let res = db
        .run_default(
            r#"
    {
        ?[y] <- [[1], [2], [3]]
        :create a {x default rand_uuid_v1() => y}
        :returning
    } as _last
    {
        ?[x] := *_last{_kind: 'inserted', x}
    }
    "#,
        )
        .expect("as-store with :returning and UUID default should succeed");
    assert_eq!(
        3,
        res.rows.len(),
        "3 inserted rows should be captured in _last"
    );
    for row in res.into_json()["rows"]
        .as_array()
        .expect("as-store result rows should be a JSON array")
    {
        println!("{}", row);
    }
    assert!(
        db.run_default(
            r#"
    {
        ?[x, x] := x = 1
    } as _last
    "#
        )
        .is_err(),
        "duplicate variable in query head should fail"
    );

    let res = db
        .run_default(
            r#"
    {
        x[y] <- [[1], [2], [3]]
        ?[sum(y)] := x[y]
    } as _last
    {
        ?[sum_y] := *_last{sum_y}
    }
    "#,
        )
        .expect("as-store with aggregate should succeed");
    assert_eq!(
        1,
        res.rows.len(),
        "sum aggregation should produce exactly 1 row"
    );
    for row in res.into_json()["rows"]
        .as_array()
        .expect("as-store aggregate result should be a JSON array")
    {
        println!("{}", row);
    }
}

#[test]
fn update_shall_not_destroy_values() {
    let db = DbInstance::default();
    db.run_default(r"?[x, y] <- [[1, 2]] :create z {x => y default 0}")
        .expect("creating relation with initial data and default should succeed");
    let r = db
        .run_default(r"?[x, y] := *z {x, y}")
        .expect("querying z should succeed");
    assert_eq!(
        r.into_json()["rows"],
        json!([[1, 2]]),
        "initial row should be [1, 2]"
    );
    db.run_default(r"?[x] <- [[1]] :update z {x}")
        .expect("update with only key should succeed");
    let r = db
        .run_default(r"?[x, y] := *z {x, y}")
        .expect("querying z after key-only update should succeed");
    assert_eq!(
        r.into_json()["rows"],
        json!([[1, 2]]),
        "key-only update should not change value y"
    );
}

#[test]
fn update_shall_work() {
    let db = DbInstance::default();
    db.run_default(r"?[x, y, z] <- [[1, 2, 3]] :create z {x => y, z}")
        .expect("creating relation z with initial data should succeed");
    let r = db
        .run_default(r"?[x, y, z] := *z {x, y, z}")
        .expect("querying z should succeed");
    assert_eq!(
        r.into_json()["rows"],
        json!([[1, 2, 3]]),
        "initial row should be [1, 2, 3]"
    );
    db.run_default(r"?[x, y] <- [[1, 4]] :update z {x, y}")
        .expect("partial update of y should succeed");
    let r = db
        .run_default(r"?[x, y, z] := *z {x, y, z}")
        .expect("querying z after partial update should succeed");
    assert_eq!(
        r.into_json()["rows"],
        json!([[1, 4, 3]]),
        "after updating y to 4, z should remain 3"
    );
}

#[test]
fn sysop_in_imperatives() {
    let script = r#"
    {
            :create cm_src {
                aid: String =>
                title: String,
                author: String?,
                kind: String,
                url: String,
                domain: String?,
                pub_time: Float?,
                dt: Float default now(),
                weight: Float default 1,
            }
        }
        {
            :create cm_txt {
                tid: String =>
                aid: String,
                tag: String,
                follows_tid: String?,
                dup_for: String?,
                text: String,
                info_amount: Int,
            }
        }
        {
            :create cm_seg {
                sid: String =>
                tid: String,
                tag: String,
                part: Int,
                text: String,
                vec: <F32; 1536>,
            }
        }
        {
            ::hnsw create cm_seg:vec {
                dim: 1536,
                m: 50,
                dtype: F32,
                fields: vec,
                distance: Cosine,
                ef: 100,
            }
        }
        {
            ::lsh create cm_txt:lsh {
                extractor: text,
                extract_filter: is_null(dup_for),
                tokenizer: NGram,
                n_perm: 200,
                target_threshold: 0.5,
                n_gram: 7,
            }
        }
        {::relations}
    "#;
    let db = DbInstance::default();
    db.run_default(script)
        .expect("complex sysop-in-imperatives script should succeed");
}

#[test]
fn bad_parse() {
    let db = DbInstance::default();
    db.run_default(
        r"
        :create named_hero_history {
        name: String,
        value: Bool,
        when: Int
    }",
    )
    .expect("creating named_hero_history relation should succeed");
    db.run_default(r"
        last_named_hero[first, first, max(hist)] := *named_hero_history[first, first, value, hist], hist <= 1;

        some_named_hero[first, first, value] := last_named_hero[first, first, last], *named_hero_history[first, first, value, last];

        named_hero[first, first, value] := cast[first], value = false, not some_named_hero[first, first, _];
        named_hero[first, first, value] := some_named_hero[first, first, value];
        ?[hero] :=
    ").expect_err("should fail");
}

#[test]
fn puts() {
    let db = DbInstance::default();
    db.run_default(
        r"
            :create cm_txt {
                tid: String =>
                aid: String,
                tag: String,
                follows_tid: String? default null,
                for_qs: [String] default [],
                dup_for: String? default null,
                text: String,
                seg_vecs: [<F32; 1536>],
                seg_pos: [(Int, Int)],
                format: String default 'text',
                info_amount: Int,
            }
    ",
    )
    .expect("creating cm_txt relation should succeed");
    db.run_default(
        r"
        ?[tid, aid, tag, text, info_amount, dup_for, seg_vecs, seg_pos] := dup_for = null,
                tid = 'x', aid = 'y', tag = 'z', text = 'w', info_amount = 12,
                follows_tid = null, for_qs = [], format = 'x',
                seg_vecs = [], seg_pos = [[0, 10]]
        :put cm_txt {tid, aid, tag, text, info_amount, seg_vecs, seg_pos, dup_for}
    ",
    )
    .expect("inserting into cm_txt should succeed");
}

#[test]
fn short_hand() {
    let db = DbInstance::default();
    db.run_default(r":create x {x => y, z}")
        .expect("creating relation x should succeed");
    db.run_default(r"?[x, y, z] <- [[1, 2, 3]] :put x {}")
        .expect("shorthand put with empty braces should succeed");
    let r = db
        .run_default(r"?[x, y, z] := *x {x, y, z}")
        .expect("querying relation x should succeed");
    assert_eq!(
        r.into_json()["rows"],
        json!([[1, 2, 3]]),
        "shorthand put should store all columns"
    );
}

#[test]
fn param_shorthand() {
    let db = DbInstance::default();
    db.run_script(
        r"
        ?[] <- [[$x, $y, $z]]
        :create x {}
    ",
        BTreeMap::from([
            ("x".to_string(), DataValue::from(1)),
            ("y".to_string(), DataValue::from(2)),
            ("z".to_string(), DataValue::from(3)),
        ]),
        ScriptMutability::Mutable,
    )
    .expect("param shorthand create should succeed");
    let res = db.run_default(r"?[x, y, z] := *x {x, y, z}");
    assert_eq!(
        res.expect("querying after param shorthand should succeed")
            .into_json()["rows"],
        json!([[1, 2, 3]]),
        "param shorthand should store all columns correctly"
    );
}

#[test]
fn crashy_imperative() {
    let db = DbInstance::default();
    db.run_default(
        r"
        {:create _test {a}}

        %loop
            %if { len[count(x)] := *_test[x]; ?[x] := len[z], x = z >= 10 }
                %then %return _test
            %end
            { ?[a] := a = rand_uuid_v1(); :put _test {a} }
        %end
        ",
    )
    .expect("imperative loop accumulating 10 rows should succeed");
}

#[test]
fn hnsw_index() {
    let db = DbInstance::default();
    db.run_default(
        r#"
        :create beliefs {
            belief_id: Uuid,
            character_id: Uuid,
            belief: String,
            last_accessed_at: Validity default [floor(now()), true],
            =>
            details: String default "",
            parent_belief_id: Uuid? default null,
            valence: Float default 0,
            aspects: [(String, Float, String, String)] default [],
            belief_embedding: <F32; 768>,
            details_embedding: <F32; 768>,
        }
        "#,
    )
    .expect("creating beliefs relation should succeed");
    db.run_default(
        r#"
        ::hnsw create beliefs:embedding_space {
            dim: 768,
            m: 50,
            dtype: F32,
            fields: [belief_embedding, details_embedding],
            distance: Cosine,
            ef_construction: 20,
            extend_candidates: false,
            keep_pruned_connections: false,
        }
    "#,
    )
    .expect("creating HNSW index on beliefs should succeed");
    db.run_default(r#"
        ?[belief_id, character_id, belief, belief_embedding, details_embedding] <- [[rand_uuid_v1(), rand_uuid_v1(), "test", rand_vec(768), rand_vec(768)]]
        :put beliefs {}
    "#).expect("inserting belief row should succeed");
    let res = db.run_default(r#"
            ?[belief, valence, dist, character_id, vector] := ~beliefs:embedding_space{ belief, valence, character_id |
                query: rand_vec(768),
                k: 100,
                ef: 20,
                radius: 1.0,
                bind_distance: dist,
                bind_vector: vector
            }

            :order -valence
            :order dist
    "#).expect("HNSW KNN query on beliefs should succeed");
    println!("{}", res.into_json()["rows"][0][4]);
}

#[test]
fn fts_drop() {
    let db = DbInstance::default();
    db.run_default(
        r#"
            :create entity {name}
        "#,
    )
    .expect("creating entity relation should succeed");
    db.run_default(
        r#"
        ::fts create entity:fts_index { extractor: name,
            tokenizer: Simple, filters: [Lowercase]
        }
    "#,
    )
    .expect("creating FTS index on entity should succeed");
    db.run_default(
        r#"
        ::fts drop entity:fts_index
    "#,
    )
    .expect("dropping FTS index should succeed");
}
