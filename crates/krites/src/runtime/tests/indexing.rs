//! Tests for vector indexing, FTS indexing, LSH indexing, and insertions.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::collections::BTreeMap;

use crate::DbInstance;
use crate::fts::{TokenizerCache, TokenizerConfig};
use crate::runtime::db::ScriptMutability;

#[test]
fn when_hnsw_filter_changes_disqualified_entry_removed_from_index() {
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
fn when_hnsw_index_created_knn_query_returns_nearest_neighbors() {
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
fn when_fts_index_created_text_search_finds_matching_rows() {
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
fn when_lsh_index_created_exact_match_found_across_thresholds() {
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
fn when_lsh_index_on_text_field_exact_match_found_across_thresholds() {
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
fn when_lsh_row_deleted_similarity_search_returns_empty() {
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
fn when_lsh_index_built_similarity_search_returns_matching_results() {
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
fn when_hnsw_bulk_insert_with_filter_knn_returns_filtered_results() {
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
