//! Integration tests for the engine runtime.
#![expect(
    clippy::expect_used,
    reason = "test assertions use .expect() for descriptive panic messages"
)]
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
        .expect("query with limit 2 should succeed")
        .into_json();
    assert_eq!(
        res["rows"],
        json!([[3], [5]]),
        "rows with limit 2 should be [3] and [5]"
    );
    let res = db
        .run_default("?[a] := a in [5,3,1,2,4] :limit 2 :offset 1")
        .expect("query with limit and offset should succeed")
        .into_json();
    assert_eq!(
        res["rows"],
        json!([[1], [3]]),
        "rows with limit 2 offset 1 should be [1] and [3]"
    );
    let res = db
        .run_default("?[a] := a in [5,3,1,2,4] :limit 2 :offset 4")
        .expect("query with limit and offset 4 should succeed")
        .into_json();
    assert_eq!(
        res["rows"],
        json!([[4]]),
        "rows with limit 2 offset 4 should be [4]"
    );
    let res = db
        .run_default("?[a] := a in [5,3,1,2,4] :limit 2 :offset 5")
        .expect("query with limit and offset 5 should succeed")
        .into_json();
    assert_eq!(
        res["rows"],
        json!([]),
        "rows with limit 2 offset 5 should be empty"
    );
}

#[test]
fn test_normal_aggr_empty() {
    let db = DbInstance::default();
    let res = db
        .run_default("?[count(a)] := a in []")
        .expect("count aggregation on empty set should succeed")
        .rows;
    assert_eq!(
        res,
        vec![vec![DataValue::from(0)]],
        "count of empty set should be 0"
    );
}

#[test]
fn test_meet_aggr_empty() {
    let db = DbInstance::default();
    let res = db
        .run_default("?[min(a)] := a in []")
        .expect("min aggregation on empty set should succeed")
        .rows;
    assert_eq!(
        res,
        vec![vec![DataValue::Null]],
        "min of empty set should be null"
    );

    let res = db
        .run_default("?[min(a), count(a)] := a in []")
        .expect("combined min and count aggregation should succeed")
        .rows;
    assert_eq!(
        res,
        vec![vec![DataValue::Null, DataValue::from(0)]],
        "min and count of empty set should be null and 0"
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
        .expect("layered sum aggregation query should succeed")
        .rows;
    assert_eq!(
        res[0][0],
        DataValue::from(21.),
        "sum of layered values 1-6 should be 21"
    )
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
    .expect("relation creation should succeed");
    debug!("real test begins");
    let res = db
        .run_default(
            r#"
        r[code, dist] := *airport{code}, *route{fr: code, dist};
        ?[dist] := r['a', dist], dist > 0.5, dist <= 1.1;
        "#,
        )
        .expect("conditional query should succeed")
        .rows;
    assert_eq!(
        res[0][0],
        DataValue::from(1.1),
        "filtered distance should be 1.1"
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
        .expect("recursive grandparent query should succeed")
        .rows;
    println!("{:?}", res);
    assert_eq!(
        res[0][0],
        DataValue::from("jakob"),
        "grandparent of joseph through abraham should be jakob"
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
    .expect("table creation with default columns should succeed");

    db.run_default(
        r#"
        ?[uid, quitted, mood] <- [['z', true, 'x']]
            :put status {uid => quitted, mood}
        "#,
    )
    .expect("insert with default timestamp column should succeed");
}

#[test]
fn rm_does_not_need_all_keys() {
    let db = DbInstance::default();
    db.run_default(":create status {uid => mood}")
        .expect("relation creation should succeed");
    assert!(
        db.run_default("?[uid, mood] <- [[1, 2]] :put status {uid => mood}",)
            .is_ok(),
        "put with all required columns should succeed"
    );
    assert!(
        db.run_default("?[uid, mood] <- [[2]] :put status {uid}",)
            .is_err(),
        "put with missing value column should fail"
    );
    assert!(
        db.run_default("?[uid, mood] <- [[3, 2]] :rm status {uid => mood}",)
            .is_ok(),
        "rm with all key columns should succeed"
    );
    assert!(
        db.run_default("?[uid] <- [[1]] :rm status {uid}").is_ok(),
        "rm with only key columns should succeed"
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
    assert!(res.is_ok(), "PageRank with wildcards should succeed");

    let db = DbInstance::default();
    let res = db.run_default(
        r#"
            r[] <- [[1, 2]]
            ?[] <~ PageRank(r[a, b])
        "#,
    );
    assert!(res.is_ok(), "PageRank with named variables should succeed");

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
        .expect("underscore wildcard query should succeed")
        .rows;
    assert_eq!(
        res.len(),
        4,
        "cross product of 2x2 relations should yield 4 rows"
    );

    let res = db.run_default(
        r#"
        ?[_] := _ = 1
        "#,
    );
    assert!(
        res.is_err(),
        "query with underscore in output position should fail"
    );

    let res = db
        .run_default(
            r#"
        ?[x] := x = 1, _ = 1, _ = 2
        "#,
        )
        .expect("query with multiple underscores should succeed")
        .rows;

    assert_eq!(
        res.len(),
        1,
        "query with underscore unification should yield 1 row"
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
        .expect("imperative script with returning should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([[2], [6], [10]]),
        "doubled odd values should be [2, 6, 10]"
    );
    let res = db.run_default(
        r#"
        {?[a] := *_xxz[b], a = b * 2}
        "#,
    );
    assert!(
        res.is_err(),
        "query on temporary relation after script should fail"
    );
}

#[test]
fn test_trigger() {
    let db = DbInstance::default();
    db.run_default(":create friends {fr: Int, to: Int => data: Any}")
        .expect("friends relation creation should succeed");
    db.run_default(":create friends.rev {to: Int, fr: Int => data: Any}")
        .expect("friends.rev relation creation should succeed");
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
    .expect("trigger setup should succeed");
    db.run_default(r"?[fr, to, data] <- [[1,2,3]] :put friends {fr, to => data}")
        .expect("put into friends should succeed");
    let ret = db
        .export_relations(["friends", "friends.rev"].into_iter())
        .expect("export_relations should succeed");
    let frs = ret
        .get("friends")
        .expect("friends relation should be present in export");
    assert_eq!(
        vec![DataValue::from(1), DataValue::from(2), DataValue::from(3)],
        frs.rows[0],
        "friends row should contain (1, 2, 3)"
    );

    let frs_rev = ret
        .get("friends.rev")
        .expect("friends.rev relation should be present in export");
    assert_eq!(
        vec![DataValue::from(2), DataValue::from(1), DataValue::from(3)],
        frs_rev.rows[0],
        "friends.rev row should contain reversed keys (2, 1, 3)"
    );
    db.run_default(r"?[fr, to] <- [[1,2], [2,3]] :rm friends {fr, to}")
        .expect("remove from friends should succeed");
    let ret = db
        .export_relations(["friends", "friends.rev"].into_iter())
        .expect("export_relations after remove should succeed");
    let frs = ret
        .get("friends")
        .expect("friends relation should be present after remove");
    assert!(
        frs.rows.is_empty(),
        "friends relation should be empty after removing all rows"
    );
}

#[test]
fn test_callback() {
    let db = DbInstance::default();
    let mut collected = vec![];
    let (_id, receiver) = db.register_callback("friends", None);
    db.run_default(":create friends {fr: Int, to: Int => data: Any}")
        .expect("friends relation creation for callback test should succeed");
    db.run_default(r"?[fr, to, data] <- [[1,2,3],[4,5,6]] :put friends {fr, to => data}")
        .expect("initial put into friends should succeed");
    db.run_default(r"?[fr, to, data] <- [[1,2,4],[4,7,6]] :put friends {fr, to => data}")
        .expect("second put into friends should succeed");
    db.run_default(r"?[fr, to] <- [[1,9],[4,5]] :rm friends {fr, to}")
        .expect("remove from friends should succeed");
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
        "first Put should have 2 new rows"
    );
    assert_eq!(
        collected[0].1.rows[0].len(),
        3,
        "first Put new row should have 3 columns"
    );
    assert_eq!(
        collected[0].2.rows.len(),
        0,
        "first Put should have no old rows"
    );
    assert_eq!(
        collected[1].0,
        CallbackOp::Put,
        "second callback should be a Put operation"
    );
    assert_eq!(
        collected[1].1.rows.len(),
        2,
        "second Put should have 2 new rows"
    );
    assert_eq!(
        collected[1].1.rows[0].len(),
        3,
        "second Put new row should have 3 columns"
    );
    assert_eq!(
        collected[1].2.rows.len(),
        1,
        "second Put should have 1 replaced old row"
    );
    assert_eq!(
        collected[1].2.rows[0],
        vec![DataValue::from(1), DataValue::from(2), DataValue::from(3)],
        "replaced old row should contain original values (1, 2, 3)"
    );
    assert_eq!(
        collected[2].0,
        CallbackOp::Rm,
        "third callback should be a Rm operation"
    );
    assert_eq!(
        collected[2].1.rows.len(),
        2,
        "Rm should report 2 requested rows"
    );
    assert_eq!(
        collected[2].1.rows[0].len(),
        2,
        "Rm requested row should have 2 columns"
    );
    assert_eq!(
        collected[2].2.rows.len(),
        1,
        "Rm should have 1 actually deleted row"
    );
    assert_eq!(
        collected[2].2.rows[0].len(),
        3,
        "Rm deleted row should have 3 columns"
    );
}

#[test]
fn test_update() {
    let db = DbInstance::default();
    db.run_default(":create friends {fr: Int, to: Int => a: Any, b: Any, c: Any}")
        .expect("friends relation creation should succeed");
    db.run_default("?[fr, to, a, b, c] <- [[1,2,3,4,5]] :put friends {fr, to => a, b, c}")
        .expect("initial put into friends should succeed");
    let res = db
        .run_default("?[fr, to, a, b, c] := *friends{fr, to, a, b, c}")
        .expect("query all friends should succeed")
        .into_json();
    assert_eq!(
        res["rows"][0],
        json!([1, 2, 3, 4, 5]),
        "initial friends row should match inserted values"
    );
    db.run_default("?[fr, to, b] <- [[1, 2, 100]] :update friends {fr, to => b}")
        .expect("update friends field should succeed");
    let res = db
        .run_default("?[fr, to, a, b, c] := *friends{fr, to, a, b, c}")
        .expect("query friends after update should succeed")
        .into_json();
    assert_eq!(
        res["rows"][0],
        json!([1, 2, 3, 100, 5]),
        "friends row after update should have b=100"
    );
}

#[test]
fn test_index() {
    let db = DbInstance::default();
    db.run_default(":create friends {fr: Int, to: Int => data: Any}")
        .expect("friends relation creation should succeed");

    db.run_default(r"?[fr, to, data] <- [[1,2,3],[4,5,6]] :put friends {fr, to, data}")
        .expect("initial data insertion into friends should succeed");

    assert!(
        db.run_default("::index create friends:rev {to, no}")
            .is_err(),
        "index creation with nonexistent column should fail"
    );
    db.run_default("::index create friends:rev {to, data}")
        .expect("index creation on friends:rev should succeed");

    db.run_default(r"?[fr, to, data] <- [[1,2,5],[6,5,7]] :put friends {fr, to => data}")
        .expect("upsert into friends should succeed");
    db.run_default(r"?[fr, to] <- [[4,5]] :rm friends {fr, to}")
        .expect("remove from friends should succeed");

    let rels_data = db
        .export_relations(["friends", "friends:rev"].into_iter())
        .expect("export_relations should succeed");
    assert_eq!(
        rels_data["friends"].clone().into_json()["rows"],
        json!([[1, 2, 5], [6, 5, 7]]),
        "friends rows should reflect upsert results"
    );
    assert_eq!(
        rels_data["friends:rev"].clone().into_json()["rows"],
        json!([[2, 5, 1], [5, 7, 6]]),
        "friends:rev index rows should reflect upsert results"
    );

    let rels = db
        .run_default("::relations")
        .expect("list relations should succeed");
    assert_eq!(
        rels.rows[1][0],
        DataValue::from("friends:rev"),
        "second relation should be the reverse index"
    );
    assert_eq!(
        rels.rows[1][1],
        DataValue::from(3),
        "reverse index should have 3 columns"
    );
    assert_eq!(
        rels.rows[1][2],
        DataValue::from("index"),
        "reverse index type should be index"
    );

    let cols = db
        .run_default("::columns friends:rev")
        .expect("list columns should succeed");
    assert_eq!(
        cols.rows.len(),
        3,
        "friends:rev index should have 3 columns"
    );

    let res = db
        .run_default("?[fr, data] := *friends:rev{to: 2, fr, data}")
        .expect("index query on friends:rev should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([[1, 5]]),
        "index query result should match expected row"
    );

    let res = db
        .run_default("?[fr, data] := *friends{to: 2, fr, data}")
        .expect("query friends using index should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([[1, 5]]),
        "friends query using index should match expected row"
    );

    let expl = db
        .run_default("::explain { ?[fr, data] := *friends{to: 2, fr, data} }")
        .expect("explain query should succeed");
    let joins = expl.into_json()["rows"]
        .as_array()
        .expect("explain result rows should be a JSON array")
        .iter()
        .map(|row| row.as_array().expect("explain row should be a JSON array")[5].clone())
        .collect_vec();
    assert!(
        joins.contains(&json!(":friends:rev")),
        "explain output should show friends:rev index usage"
    );
    db.run_default("::index drop friends:rev")
        .expect("drop index should succeed");
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
        .expect("custom fixed rule registration should succeed");
    let res = db
        .run_default(
            r#"
        rel[] <- [[1,2,3,4],[5,6,7,8]]
        ?[x] <~ SumCols(rel[], mult: 100)
    "#,
        )
        .expect("query using custom SumCols rule should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([[1000], [2600]]),
        "SumCols with mult 100 should yield [1000] and [2600]"
    );
}

#[test]
fn test_index_short() {
    let db = DbInstance::default();
    db.run_default(":create friends {fr: Int, to: Int => data: Any}")
        .expect("friends relation creation should succeed");

    db.run_default(r"?[fr, to, data] <- [[1,2,3],[4,5,6]] :put friends {fr, to => data}")
        .expect("initial data insertion into friends should succeed");

    db.run_default("::index create friends:rev {to}")
        .expect("short index creation on friends:rev should succeed");

    db.run_default(r"?[fr, to, data] <- [[1,2,5],[6,5,7]] :put friends {fr, to => data}")
        .expect("upsert into friends should succeed");
    db.run_default(r"?[fr, to] <- [[4,5]] :rm friends {fr, to}")
        .expect("remove from friends should succeed");

    let rels_data = db
        .export_relations(["friends", "friends:rev"].into_iter())
        .expect("export_relations should succeed");
    assert_eq!(
        rels_data["friends"].clone().into_json()["rows"],
        json!([[1, 2, 5], [6, 5, 7]]),
        "friends rows should reflect upsert results"
    );
    assert_eq!(
        rels_data["friends:rev"].clone().into_json()["rows"],
        json!([[2, 1], [5, 6]]),
        "short friends:rev index rows should reflect upsert results"
    );

    let rels = db
        .run_default("::relations")
        .expect("list relations should succeed");
    assert_eq!(
        rels.rows[1][0],
        DataValue::from("friends:rev"),
        "second relation should be the reverse index"
    );
    assert_eq!(
        rels.rows[1][1],
        DataValue::from(2),
        "short reverse index should have 2 columns"
    );
    assert_eq!(
        rels.rows[1][2],
        DataValue::from("index"),
        "reverse index type should be index"
    );

    let cols = db
        .run_default("::columns friends:rev")
        .expect("list columns should succeed");
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
        .map(|row| row.as_array().expect("explain row should be a JSON array")[5].clone())
        .collect_vec();
    assert!(
        joins.contains(&json!(":friends:rev")),
        "explain output should show friends:rev index usage"
    );

    let res = db
        .run_default("?[fr, data] := *friends{to: 2, fr, data}")
        .expect("index query should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([[1, 5]]),
        "index query result should match expected row"
    );
}

#[test]
fn test_multi_tx() {
    let db = DbInstance::default();
    let tx = db.multi_transaction_test(true);
    tx.run_script(":create a {a}", Default::default())
        .expect("create table in transaction should succeed");
    tx.run_script("?[a] <- [[1]] :put a {a}", Default::default())
        .expect("put row 1 in transaction should succeed");
    assert!(
        tx.run_script(":create a {a}", Default::default()).is_err(),
        "duplicate table creation in transaction should fail"
    );
    tx.run_script("?[a] <- [[2]] :put a {a}", Default::default())
        .expect("put row 2 in transaction should succeed");
    tx.run_script("?[a] <- [[3]] :put a {a}", Default::default())
        .expect("put row 3 in transaction should succeed");
    tx.commit().expect("transaction commit should succeed");
    assert_eq!(
        db.run_default("?[a] := *a[a]")
            .expect("query after commit should succeed")
            .into_json()["rows"],
        json!([[1], [2], [3]]),
        "committed transaction should persist all 3 rows"
    );

    let db = DbInstance::default();
    let tx = db.multi_transaction_test(true);
    tx.run_script(":create a {a}", Default::default())
        .expect("create table in second transaction should succeed");
    tx.run_script("?[a] <- [[1]] :put a {a}", Default::default())
        .expect("put row 1 in second transaction should succeed");
    assert!(
        tx.run_script(":create a {a}", Default::default()).is_err(),
        "duplicate table creation in second transaction should fail"
    );
    tx.run_script("?[a] <- [[2]] :put a {a}", Default::default())
        .expect("put row 2 in second transaction should succeed");
    tx.run_script("?[a] <- [[3]] :put a {a}", Default::default())
        .expect("put row 3 in second transaction should succeed");
    tx.abort().expect("transaction abort should succeed");
    assert!(
        db.run_default("?[a] := *a[a]").is_err(),
        "query after aborted transaction should fail because table does not exist"
    );
}

#[test]
fn test_vec_types() {
    let db = DbInstance::default();
    db.run_default(":create a {k: String => v: <F32; 8>}")
        .expect("vector type relation creation should succeed");
    db.run_default("?[k, v] <- [['k', [1,2,3,4,5,6,7,8]]] :put a {k => v}")
        .expect("vector data insertion should succeed");
    let res = db
        .run_default("?[k, v] := *a{k, v}")
        .expect("vector query should succeed");
    assert_eq!(
        json!([1., 2., 3., 4., 5., 6., 7., 8.]),
        res.into_json()["rows"][0][1],
        "stored F32 vector should match inserted values"
    );
    let res = db
        .run_default("?[v] <- [[vec([1,2,3,4,5,6,7,8])]]")
        .expect("vec() constructor query should succeed");
    assert_eq!(
        json!([1., 2., 3., 4., 5., 6., 7., 8.]),
        res.into_json()["rows"][0][0],
        "vec() constructor result should match input values"
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
        .expect("vector distance computation query should succeed");
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
    .expect("vector relation creation should succeed");
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
    .expect("HNSW index creation should succeed");
    let res = db
        .run_default("?[k] := *a:vec{layer: 0, fr_k, to_k}, k = fr_k or k = to_k")
        .expect("HNSW graph query should succeed");
    assert_eq!(
        res.rows.len(),
        1,
        "HNSW graph should have 1 node after filtered insert"
    );
    println!("update!");
    db.run_default(r#"?[k, m] <- [["a", false]] :update a {}"#)
        .expect("update to disable filter should succeed");
    let res = db
        .run_default("?[k] := *a:vec{layer: 0, fr_k, to_k}, k = fr_k or k = to_k")
        .expect("HNSW graph query after filter update should succeed");
    assert_eq!(
        res.rows.len(),
        0,
        "HNSW graph should be empty after filter disables the node"
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
    .expect("vector relation creation with multiple entries should succeed");
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
    .expect("HNSW index creation for vec should succeed");
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
    .expect("additional vector insertions should succeed");

    println!("all links");
    for (_, nrows) in db
        .export_relations(["a:vec"].iter())
        .expect("export HNSW graph relations should succeed")
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
        .expect("KNN query on HNSW index should succeed");
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
        .expect("FTS relation creation should succeed");
    db.run_default(
        r"?[k, v] <- [['a', 'hello world!'], ['b', 'the world is round']] :put a {k => v}",
    )
    .expect("initial FTS data insertion should succeed");
    db.run_default(
        r"::fts create a:fts {
            extractor: v,
            tokenizer: Simple,
            filters: [Lowercase, Stemmer('English'), Stopwords('en')]
        }",
    )
    .expect("FTS index creation should succeed");
    db.run_default(
        r"?[k, v] <- [
            ['b', 'the world is square!'],
            ['c', 'see you at the end of the world!'],
            ['d', 'the world is the world and makes the world go around']
        ] :put a {k => v}",
    )
    .expect("additional FTS data insertion should succeed");
    let res = db
        .run_default(
            r"
        ?[word, src_k, offset_from, offset_to, position, total_length] :=
            *a:fts{word, src_k, offset_from, offset_to, position, total_length}
        ",
        )
        .expect("FTS index data query should succeed");
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
        .expect("FTS search result rows should be a JSON array")
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
            .expect("LSH relation creation should succeed");
        db.run_script(
            r"::lsh create a:lsh {extractor: v, tokenizer: NGram, n_gram: 3, target_threshold: $t }",
            BTreeMap::from([("t".into(), f.into())]),
            ScriptMutability::Mutable
        )
            .expect("LSH index creation should succeed");
        db.run_default("?[k, v] <- [['a', 'ewiygfspeoighjsfcfxzdfncalsdf']] :put a {k => v}")
            .expect("LSH data insertion should succeed");
        let res = db
            .run_default("?[k] := ~a:lsh{k | query: 'ewiygfspeoighjsfcfxzdfncalsdf', k: 1}")
            .expect("LSH similarity search should succeed");
        assert!(
            !res.rows.is_empty(),
            "LSH similarity search should return at least one result"
        );
    }
}

#[test]
fn test_lsh_indexing3() {
    for i in 1..10 {
        let f = i as f64 / 10.;
        let db = DbInstance::default();
        db.run_default(r":create text {id: String,  => text: String, url: String? default null, dt: Float default now(), dup_for: String? default null }")
            .expect("text relation creation should succeed");
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
        .expect("LSH index creation on text should succeed");
        db.run_default(
            "?[id, text] <- [['a', 'This function first generates 32 random bytes using the os.urandom function. It then base64 encodes these bytes using base64.urlsafe_b64encode, removes the padding, and decodes the result to a string.']] :put text {id, text}",
        )
        .expect("text data insertion should succeed");
        let res = db
            .run_default(
                r#"?[id, dup_for] :=
    ~text:lsh{id: id, dup_for: dup_for, | query: "This function first generates 32 random bytes using the os.urandom function. It then base64 encodes these bytes using base64.urlsafe_b64encode, removes the padding, and decodes the result to a string.", }"#,
            )
            .expect("LSH similarity search on text should succeed");
        assert!(
            !res.rows.is_empty(),
            "LSH similarity search on text should return at least one result"
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
        .expect("single-key relation query should succeed");
    assert_eq!(
        0,
        res.rows.len(),
        "filtered query with mismatched condition should return no rows"
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
        .expect("multi-key relation query should succeed");
    assert_eq!(
        0,
        res.rows.len(),
        "filtered query with mismatched condition should return no rows"
    );
}

#[test]
fn test_lsh_indexing4() {
    for i in 1..10 {
        let f = i as f64 / 10.;
        let db = DbInstance::default();
        db.run_default(r":create a {k: String => v: String}")
            .expect("LSH relation creation should succeed");
        db.run_script(
            r"::lsh create a:lsh {extractor: v, tokenizer: NGram, n_gram: 3, target_threshold: $t }",
            BTreeMap::from([("t".into(), f.into())]),
            ScriptMutability::Mutable
        )
            .expect("LSH index creation should succeed");
        db.run_default("?[k, v] <- [['a', 'ewiygfspeoighjsfcfxzdfncalsdf']] :put a {k => v}")
            .expect("LSH data insertion should succeed");
        db.run_default("?[k] <- [['a']] :rm a {k}")
            .expect("remove LSH entry should succeed");
        let res = db
            .run_default("?[k] := ~a:lsh{k | query: 'ewiygfspeoighjsfcfxzdfncalsdf', k: 1}")
            .expect("LSH search after removal should succeed");
        assert!(
            res.rows.is_empty(),
            "LSH search after removal should return no results"
        );
    }
}

#[test]
fn test_lsh_indexing() {
    let db = DbInstance::default();
    db.run_default(r":create a {k: String => v: String}")
        .expect("LSH relation creation should succeed");
    db.run_default(
        r"?[k, v] <- [['a', 'hello world!'], ['b', 'the world is round']] :put a {k => v}",
    )
    .expect("initial LSH data insertion should succeed");
    db.run_default(
        r"::lsh create a:lsh {extractor: v, tokenizer: Simple, n_gram: 3, target_threshold: 0.3 }",
    )
    .expect("LSH index creation should succeed");
    db.run_default(
        r"?[k, v] <- [
            ['b', 'the world is square!'],
            ['c', 'see you at the end of the world!'],
            ['d', 'the world is the world and makes the world go around'],
            ['e', 'the world is the world and makes the world not go around']
        ] :put a {k => v}",
    )
    .expect("additional LSH data insertion should succeed");
    let res = db
        .run_default("::columns a:lsh")
        .expect("list LSH columns should succeed");
    for row in res.into_json()["rows"]
        .as_array()
        .expect("LSH columns rows should be a JSON array")
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
        .expect("LSH hash query should succeed");
    let _res = db
        .run_default(
            r"
        ?[k, minhash] :=
            *a:lsh:inv{k, minhash}
        ",
        )
        .expect("LSH inverse index query should succeed");
    let res = db
        .run_default(
            r"
            ?[k, v] := ~a:lsh{k, v |
                query: 'see him at the end of the world',
            }
            ",
        )
        .expect("LSH similarity query should succeed");
    for row in res.into_json()["rows"]
        .as_array()
        .expect("LSH similarity result rows should be a JSON array")
    {
        println!("{}", row);
    }
    let res = db
        .run_default("::indices a")
        .expect("list indices should succeed");
    for row in res.into_json()["rows"]
        .as_array()
        .expect("indices rows should be a JSON array")
    {
        println!("{}", row);
    }
    db.run_default(r"::lsh drop a:lsh")
        .expect("drop LSH index should succeed");
}

#[test]
fn test_insertions() {
    let db = DbInstance::default();
    db.run_default(r":create a {k => v: <F32; 1536> default rand_vec(1536)}")
        .expect("vector relation creation should succeed");
    db.run_default(r"?[k] <- [[1]] :put a {k}")
        .expect("initial row insertion should succeed");
    db.run_default(r"?[k, v] := *a{k, v}")
        .expect("verify initial insertion should succeed");
    db.run_default(
        r"::hnsw create a:i {
            fields: [v], dim: 1536, ef: 16, filter: k % 3 == 0,
            m: 32
        }",
    )
    .expect("HNSW index creation should succeed");
    db.run_default(r"?[count(fr_k)] := *a:i{fr_k}")
        .expect("count HNSW nodes should succeed");
    db.run_default(r"?[k] <- [[1]] :put a {k}")
        .expect("re-insert row should succeed");
    db.run_default(r"?[k] := k in int_range(300) :put a {k}")
        .expect("bulk row insertion should succeed");
    let res = db
        .run_default(
            r"?[dist, k] := ~a:i{k | query: v, bind_distance: dist, k:10, ef: 50, filter: k % 2 == 0, radius: 245}, *a{k: 96, v}",
        )
        .expect("HNSW KNN query should succeed");
    println!("results");
    for row in res.into_json()["rows"]
        .as_array()
        .expect("KNN result rows should be a JSON array")
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
        .expect("simple tokenizer retrieval should succeed");

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
    .expect("product relation creation should succeed");
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
    .expect("multi-field HNSW index creation should succeed");
    db.run_default(
        r#"
        ?[id, name, description, price, name_vec, description_vec] <- [[1, "name", "description", 100, [1], [1]]]

        :put product {id => name, description, price, name_vec, description_vec}
        "#,
    ).expect("product data insertion should succeed");
    let res = db
        .run_default("::indices product")
        .expect("list product indices should succeed");
    for row in res.into_json()["rows"]
        .as_array()
        .expect("product indices rows should be a JSON array")
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
    .expect("ensure_not idempotent operation should succeed");
}

#[test]
fn insertion() {
    let db = DbInstance::default();
    db.run_default(r":create a {x => y}")
        .expect("relation creation should succeed");
    assert!(
        db.run_default(r"?[x, y] <- [[1, 2]] :insert a {x => y}",)
            .is_ok(),
        "insert of new row should succeed"
    );
    assert!(
        db.run_default(r"?[x, y] <- [[1, 3]] :insert a {x => y}",)
            .is_err(),
        "insert of duplicate key should fail"
    );
}

#[test]
fn deletion() {
    let db = DbInstance::default();
    db.run_default(r":create a {x => y}")
        .expect("relation creation should succeed");
    assert!(
        db.run_default(r"?[x] <- [[1]] :delete a {x}").is_err(),
        "delete of non-existent row should fail"
    );
    assert!(
        db.run_default(r"?[x, y] <- [[1, 2]] :insert a {x => y}",)
            .is_ok(),
        "insert of new row should succeed"
    );
    db.run_default(r"?[x] <- [[1]] :delete a {x}")
        .expect("delete existing row should succeed");
}

#[test]
fn into_payload() {
    let db = DbInstance::default();
    db.run_default(r":create a {x => y}")
        .expect("relation creation should succeed");
    db.run_default(r"?[x, y] <- [[1, 2], [3, 4]] :insert a {x => y}")
        .expect("data insertion should succeed");

    let mut res = db
        .run_default(r"?[x, y] := *a[x, y]")
        .expect("query all rows should succeed");
    assert_eq!(res.rows.len(), 2, "query should return 2 inserted rows");

    let delete = res.clone().into_payload("a", "rm");
    db.run_script(delete.0.as_str(), delete.1, ScriptMutability::Mutable)
        .expect("delete via payload should succeed");
    assert_eq!(
        db.run_default(r"?[x, y] := *a[x, y]")
            .expect("query after delete should succeed")
            .rows
            .len(),
        0,
        "all rows should be deleted via rm payload"
    );

    db.run_default(r":create b {m => n}")
        .expect("second relation creation should succeed");
    res.headers = vec!["m".into(), "n".into()];
    let put = res.into_payload("b", "put");
    db.run_script(put.0.as_str(), put.1, ScriptMutability::Mutable)
        .expect("put via payload should succeed");
    assert_eq!(
        db.run_default(r"?[m, n] := *b[m, n]")
            .expect("query after put should succeed")
            .rows
            .len(),
        2,
        "both rows should be inserted via put payload"
    );
}

#[test]
fn returning() {
    let db = DbInstance::default();
    db.run_default(":create a {x => y}")
        .expect("relation creation should succeed");
    let res = db
        .run_default(r"?[x, y] <- [[1, 2]] :insert a {x => y} ")
        .expect("insert with returning should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([["OK"]]),
        "insert without returning should return OK"
    );

    let res = db
        .run_default(r"?[x, y] <- [[1, 3], [2, 4]] :returning :put a {x => y} ")
        .expect("put with returning should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([["inserted", 1, 3], ["inserted", 2, 4], ["replaced", 1, 2]]),
        "put returning should show inserted and replaced rows"
    );

    let res = db
        .run_default(r"?[x] <- [[1], [4]] :returning :rm a {x} ")
        .expect("rm with returning should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([
            ["requested", 1, null],
            ["requested", 4, null],
            ["deleted", 1, 3]
        ]),
        "rm returning should show requested and deleted rows"
    );
    db.run_default(r":create todo{id:Uuid default rand_uuid_v1() => label: String, done: Bool}")
        .expect("todo relation creation should succeed");
    let res = db
        .run_default(r"?[label,done] <- [['milk',false]] :put todo{label,done} :returning")
        .expect("put with returning into todo should succeed");
    assert_eq!(
        res.rows[0].len(),
        4,
        "returning row for todo should have 4 columns including uuid"
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
        .expect("or expression should parse correctly");
    db.run_default(r#"?[C] := C = 1  orx[C] := C = 1"#)
        .expect("orx identifier should parse correctly");
    db.run_default(r#"?[C] := C = true, C  inx[C] := C = 1"#)
        .expect("inx identifier should parse correctly");
    db.run_default(r#"?[k] := k in int_range(300)"#)
        .expect("int_range query should parse correctly");
    db.run_default(r#"ywcc[a] <- [[1]] noto[A] := ywcc[A] ?[A] := noto[A]"#)
        .expect("noto identifier should parse correctly");
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
        .expect("as-store script should succeed");
    assert_eq!(
        res.into_json()["rows"],
        json!([[1, 2, 3], [4, 5, 6]]),
        "as-store script should return stored rows"
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
        .expect("as-store script with create and returning should succeed");
    assert_eq!(
        3,
        res.rows.len(),
        "inserted 3 rows via UUID default should yield 3 results"
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
        "duplicate output variable in as-store block should fail"
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
        .expect("as-store script with aggregation should succeed");
    assert_eq!(
        1,
        res.rows.len(),
        "sum aggregation should return exactly 1 row"
    );
    for row in res.into_json()["rows"]
        .as_array()
        .expect("aggregation result rows should be a JSON array")
    {
        println!("{}", row);
    }
}

#[test]
fn update_shall_not_destroy_values() {
    let db = DbInstance::default();
    db.run_default(r"?[x, y] <- [[1, 2]] :create z {x => y default 0}")
        .expect("create with default value should succeed");
    let r = db
        .run_default(r"?[x, y] := *z {x, y}")
        .expect("query before update should succeed");
    assert_eq!(
        r.into_json()["rows"],
        json!([[1, 2]]),
        "initial row should match inserted values"
    );
    db.run_default(r"?[x] <- [[1]] :update z {x}")
        .expect("update with only key should succeed");
    let r = db
        .run_default(r"?[x, y] := *z {x, y}")
        .expect("query after update should succeed");
    assert_eq!(
        r.into_json()["rows"],
        json!([[1, 2]]),
        "update with only key should not change value"
    );
}

#[test]
fn update_shall_work() {
    let db = DbInstance::default();
    db.run_default(r"?[x, y, z] <- [[1, 2, 3]] :create z {x => y, z}")
        .expect("create with multiple values should succeed");
    let r = db
        .run_default(r"?[x, y, z] := *z {x, y, z}")
        .expect("query before update should succeed");
    assert_eq!(
        r.into_json()["rows"],
        json!([[1, 2, 3]]),
        "initial row should match inserted values"
    );
    db.run_default(r"?[x, y] <- [[1, 4]] :update z {x, y}")
        .expect("partial update should succeed");
    let r = db
        .run_default(r"?[x, y, z] := *z {x, y, z}")
        .expect("query after partial update should succeed");
    assert_eq!(
        r.into_json()["rows"],
        json!([[1, 4, 3]]),
        "partial update should change only y field"
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
        .expect("complex imperative script with sys ops should succeed");
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
    .expect("named_hero_history table creation should succeed");
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
    .expect("cm_txt relation creation should succeed");
    db.run_default(
        r"
        ?[tid, aid, tag, text, info_amount, dup_for, seg_vecs, seg_pos] := dup_for = null,
                tid = 'x', aid = 'y', tag = 'z', text = 'w', info_amount = 12,
                follows_tid = null, for_qs = [], format = 'x',
                seg_vecs = [], seg_pos = [[0, 10]]
        :put cm_txt {tid, aid, tag, text, info_amount, seg_vecs, seg_pos, dup_for}
    ",
    )
    .expect("cm_txt data insertion should succeed");
}

#[test]
fn short_hand() {
    let db = DbInstance::default();
    db.run_default(r":create x {x => y, z}")
        .expect("relation creation should succeed");
    db.run_default(r"?[x, y, z] <- [[1, 2, 3]] :put x {}")
        .expect("shorthand put should succeed");
    let r = db
        .run_default(r"?[x, y, z] := *x {x, y, z}")
        .expect("shorthand query should succeed");
    assert_eq!(
        r.into_json()["rows"],
        json!([[1, 2, 3]]),
        "shorthand put should store row correctly"
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
    .expect("param shorthand script should succeed");
    let res = db.run_default(r"?[x, y, z] := *x {x, y, z}");
    assert_eq!(
        res.expect("param shorthand query should succeed")
            .into_json()["rows"],
        json!([[1, 2, 3]]),
        "param shorthand should bind params correctly"
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
    .expect("loop imperative script should succeed");
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
    .expect("beliefs relation creation should succeed");
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
    .expect("beliefs HNSW index creation should succeed");
    db.run_default(r#"
        ?[belief_id, character_id, belief, belief_embedding, details_embedding] <- [[rand_uuid_v1(), rand_uuid_v1(), "test", rand_vec(768), rand_vec(768)]]
        :put beliefs {}
    "#).expect("beliefs data insertion should succeed");
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
    "#).expect("beliefs HNSW KNN query should succeed");
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
    .expect("entity relation creation should succeed");
    db.run_default(
        r#"
        ::fts create entity:fts_index { extractor: name,
            tokenizer: Simple, filters: [Lowercase]
        }
    "#,
    )
    .expect("FTS index creation should succeed");
    db.run_default(
        r#"
        ::fts drop entity:fts_index
    "#,
    )
    .expect("FTS index drop should succeed");
}
