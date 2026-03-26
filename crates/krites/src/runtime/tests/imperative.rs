//! Tests for imperative scripts, parser edge cases, deletions, and returning relations.
#![expect(clippy::expect_used, reason = "test assertions")]
use std::collections::BTreeMap;

use serde_json::json;

use crate::DbInstance;
use crate::data::value::DataValue;
use crate::runtime::db::ScriptMutability;

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
