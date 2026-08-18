#!/usr/bin/env bash
# Regenerate every derived krites artifact, in the one order that is correct.
#
# WHY a script rather than a line in a doc: the ordering is not guessable and
# getting it wrong produces a green local run and a red CI one.
#
#   1. `cargo fmt` FIRST. `verbatim_pct` is computed from file bytes, so
#      formatting moves it. Regenerating before formatting records figures for
#      a tree that is about to change, and `check-krites-provenance.py` then
#      fails on a mismatch that looks like a provenance problem and is not.
#   2. `measure-krites-provenance.py` next -- it writes PROVENANCE.toml AND
#      renders NOTICE.md from it, so the notice can never disagree with the
#      ledger it summarises. It also stamps each derived/dual file's MPL
#      Exhibit A header and strips it from sovereign files, so this step edits
#      SOURCE, not only artifacts. That is safe after `cargo fmt` and moves no
#      figure: the generated block is excluded from every verbatim measurement.
#   3. The three module-dag variants last. CI checks all three; regenerating
#      one and forgetting the others is the most common way to fail that step.
#
# Use after ANY merge that touched crates/krites, and after any source edit
# there. `.gitattributes` marks the derived artifacts `-merge`, so a merge
# leaves them at our side wholesale rather than with conflict markers -- which
# is what makes running this afterwards sufficient, and what stops a regenerate
# from reading a half-merged ledger (aletheia#6703: an unparsable ledger used to
# silently rewrite every graduated row as `derived`).
#
# NOTE: CAPABILITY_MATRIX.toml is deliberately NOT here. It is hand-maintained
# and only checked, never generated -- there is nothing to regenerate, and a
# merge conflict in it needs a human deciding which rows are right.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

echo "regen-krites-artifacts: cargo fmt (moves verbatim_pct, so it goes first) ..."
cargo fmt --all

echo "regen-krites-artifacts: PROVENANCE.toml + NOTICE.md ..."
python3 scripts/measure-krites-provenance.py

echo "regen-krites-artifacts: module-dag (all three variants) ..."
python3 scripts/krites-module-dag.py --out crates/krites/module-dag.json
python3 scripts/krites-module-dag.py --format markdown --out crates/krites/module-dag.md
python3 scripts/krites-module-dag.py --wave-scope --out crates/krites/module-dag.wave-scope.json

echo "regen-krites-artifacts: done. Verify with scripts/run-gate-coverage.py"
