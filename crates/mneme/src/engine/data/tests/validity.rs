//! Tests for temporal validity ranges.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::env;

use serde_json::json;

use crate::engine::DbInstance;
use crate::engine::data::value::DataValue;

#[test]
fn temporal_validity_ranges_assert_retract_and_query_correctly() {
    let path = "_test_validity";
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir_all(path);
    let db_kind = env::var("MNEME_TEST_DB_ENGINE").unwrap_or("mem".to_string());
    println!("Using {} engine", db_kind);
    let db = DbInstance::default();

    db.run_default(":create vld {a, v: Validity => d}")
        .expect("test assertion");

    assert!(
        db.run_default(
            r#"
    ?[a, v, d] <- [[1, [9223372036854775807, true], null]]
    :put vld {a, v => d}
    "#,
        )
        .is_err()
    );

    assert!(
        db.run_default(
            r#"
    ?[a, v, d] <- [[1, [-9223372036854775808, true], null]]
    :put vld {a, v => d}
    "#,
        )
        .is_err()
    );

    db.run_default(
        r#"
    ?[a, v, d] <- [[1, [0, true], 0]]
    :put vld {a, v => d}
    "#,
    )
    .expect("test assertion");

    let res = db
        .run_default(
            r#"
        ?[a, v, d] := *vld{a, v, d @ "NOW"}
    "#,
        )
        .expect("test assertion")
        .rows;
    assert_eq!(res.len(), 1);

    let res = db
        .run_default(
            r#"
        ?[a, v, d] := *vld{a, v, d}
    "#,
        )
        .expect("test assertion")
        .rows;
    assert_eq!(res.len(), 1);

    db.run_default(
        r#"
    ?[a, v, d] <- [[1, [1, false], 1]]
    :put vld {a, v => d}
    "#,
    )
    .expect("test assertion");

    let res = db
        .run_default(
            r#"
        ?[a, v, d] := *vld{a, v, d @ "NOW"}
    "#,
        )
        .expect("test assertion")
        .rows;
    assert_eq!(res.len(), 0);

    let res = db
        .run_default(
            r#"
        ?[a, v, d] := *vld{a, v, d}
    "#,
        )
        .expect("test assertion")
        .rows;
    assert_eq!(res.len(), 2);

    db.run_default(
        r#"
    ?[a, v, d] <- [[1, "ASSERT", 2]]
    :put vld {a, v => d}
    "#,
    )
    .expect("test assertion");

    let res = db
        .run_default(
            r#"
        ?[a, v, d] := *vld{a, v, d @ "NOW"}
    "#,
        )
        .expect("test assertion")
        .rows;
    assert_eq!(res.len(), 1);
    assert_eq!(res[0][2].get_int().expect("test assertion"), 2);

    let res = db
        .run_default(
            r#"
        ?[a, v, d] := *vld{a, v, d}
    "#,
        )
        .expect("test assertion")
        .rows;
    assert_eq!(res.len(), 3);

    db.run_default(
        r#"
    ?[a, v, d] <- [[1, "RETRACT", 3]]
    :put vld {a, v => d}
    "#,
    )
    .expect("test assertion");

    let res = db
        .run_default(
            r#"
        ?[a, v, d] := *vld{a, v, d @ "NOW"}
    "#,
        )
        .expect("test assertion")
        .rows;
    assert_eq!(res.len(), 0);

    let res = db
        .run_default(
            r#"
        ?[a, v, d] := *vld{a, v, d}
    "#,
        )
        .expect("test assertion")
        .rows;
    assert_eq!(res.len(), 4);
    db.run_default(
        r#"
    ?[a, v, d] <- [[1, [9223372036854775806, true], null]]
    :put vld {a, v => d}
    "#,
    )
    .expect("test assertion");

    let res = db
        .run_default(
            r#"
        ?[a, v, d] := *vld{a, v, d @ "NOW"}
    "#,
        )
        .expect("test assertion")
        .rows;
    assert_eq!(res.len(), 0);

    let res = db
        .run_default(
            r#"
        ?[a, v, d] := *vld{a, v, d @ "END"}
    "#,
        )
        .expect("test assertion")
        .rows;
    assert_eq!(res.len(), 1);
    assert_eq!(res[0][2], DataValue::Null);

    let res = db
        .run_default(
            r#"
        ?[a, v, d] := *vld{a, v, d}
    "#,
        )
        .expect("test assertion")
        .rows;
    assert_eq!(res.len(), 5);

    println!("{}", json!(res));
}
