//! Tests for expression evaluation.
#![expect(clippy::expect_used, reason = "test assertions")]
use crate::{DataValue, DbInstance};

#[test]
fn expression_eval() {
    let db = DbInstance::default();

    let res = db
        .run_default(
            r#"
    ?[a] := a = if(2 + 3 > 1 * 99999, 190291021 + 14341234212 / 2121)
    "#,
        )
        .expect("test assertion");
    assert_eq!(res.rows[0][0], DataValue::Null);

    let res = db
        .run_default(
            r#"
    ?[a] := a = if(2 + 3 > 1, true, false)
    "#,
        )
        .expect("test assertion");
    assert!(res.rows[0][0].get_bool().expect("test assertion"));
}
