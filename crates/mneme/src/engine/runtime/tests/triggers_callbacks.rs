//! Tests for triggers, callbacks, updates, indexes, and multi-transaction behavior.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::collections::BTreeMap;
use std::time::Duration;

use compact_str::CompactString;
use itertools::Itertools;
use serde_json::json;

use crate::engine::DbInstance;
use crate::engine::data::expr::Expr;
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::DataValue;
use crate::engine::error::InternalResult as Result;
use crate::engine::fixed_rule::{FixedRule, FixedRulePayload};
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::callback::CallbackOp;
use crate::engine::runtime::db::Poison;
use crate::engine::runtime::temp_store::RegularTempStore;

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
