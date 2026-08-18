// krites-exhibit-a: begin (generated -- scripts/measure-krites-provenance.py)
// This Source Code Form is subject to the terms of the Mozilla Public License,
// v. 2.0. If a copy of the MPL was not distributed with this file, You can
// obtain one at https://mozilla.org/MPL/2.0/.
// krites-exhibit-a: end

//! Tests for expression evaluation.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions: index into known-shape NamedRows results"
)]
use crate::{DataValue, DbInstance};

#[test]
fn expression_eval() {
    let db = DbInstance::default();

    let res = db
        .run_default(
            r"
    ?[a] := a = if(2 + 3 > 1 * 99999, 190291021 + 14341234212 / 2121)
    ",
        )
        .expect("test assertion");
    assert_eq!(res.rows[0][0], DataValue::Null);

    let res = db
        .run_default(
            r"
    ?[a] := a = if(2 + 3 > 1, true, false)
    ",
        )
        .expect("test assertion");
    assert!(res.rows[0][0].get_bool().expect("test assertion"));
}
