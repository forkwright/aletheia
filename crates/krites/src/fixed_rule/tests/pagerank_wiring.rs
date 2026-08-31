//! Wiring gate for the sovereign `PageRank` fixed-rule shell (#7042).
//!
//! The #7002 defect class is a finished capability that production never
//! reaches: the sovereign shell sat unwired on a dead branch once, and then
//! sat behind an off-by-default feature during its dual soak. This test
//! pins the wiring itself — it drives the real production call path
//! (`run_script` → compile → `DEFAULT_FIXED_RULES["PageRank"]` → the live
//! shell) and asserts behaviour only the sovereign shell has, so it fails
//! if the `PageRank` re-export is ever pointed back at a shell that lacks
//! it. A test that merely constructed the sovereign type directly would
//! stay green through exactly the unwiring this one exists to catch.

#![cfg(test)]
#![expect(clippy::expect_used, reason = "test assertions")]

use crate::DbInstance;

/// The sovereign shell polls `poison.check()` while emitting rows; the
/// retired CozoDB-derived shell never polled it at all (its `run` carried
/// `#[expect(unused_variables)]` on `poison`). A query whose `:timeout`
/// deadline has already elapsed when the fixed rule runs therefore aborts
/// on the sovereign shell and completes on the derived one — nothing else
/// in this pure-fixed-rule program polls the poison (verified while this
/// test was written against the derived shell: it returned 3 rows).
///
/// Deterministic, not a race: `:timeout` arms the deadline before
/// compilation begins, so it is long elapsed by the time the shell runs;
/// the assertion is that the shell OBSERVES it, not that a timer wins.
#[test]
fn pagerank_live_path_reaches_the_sovereign_shell() {
    const PROGRAM: &str = "edges[src, dst] <- [[1, 2], [2, 3], [3, 1], [2, 1], [3, 2], [1, 3]]\n\
                           ?[node, rank] <~ PageRank(edges[], iterations: 50)";
    let db = DbInstance::default();

    // First establish that the program itself is sound — otherwise a renamed
    // or unregistered rule would fail the poisoned run below for the wrong
    // reason and this test would certify wiring it never exercised.
    let ok = db
        .run_default(PROGRAM)
        .expect("PageRank program must run on the live path");
    assert_eq!(ok.rows.len(), 3, "one PageRank row per node");

    let poisoned = format!("{PROGRAM}\n:timeout 0.000000001");
    match db.run_default(&poisoned) {
        Ok(_) => panic!(
            "PageRank ran to completion under an already-elapsed :timeout deadline: \
             the live shell is not polling the query poison, which means the \
             production call path no longer reaches the sovereign shell \
             (fixed_rule/algos/pagerank_native.rs) — see #7042 and the #7002 \
             finished-but-unwired class"
        ),
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("killed"),
                "the poisoned PageRank run failed, but not from the shell \
                 observing the poison — got: {msg}"
            );
        }
    }
}
