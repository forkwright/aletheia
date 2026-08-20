#!/usr/bin/env python3
"""Behavioral tests for the Krites capability matrix and evidence parsers.

The matrix's new enforcement is only worth its cost if the failure modes it
claims to catch actually fail. Each test below stages one of them against the
LIVE matrix rows and asserts a specific error, so a future refactor that
silently turns a check into a no-op is caught here rather than by nobody:

  a gate_test absent from authoritative nextest output -> error
  a gate_test marked ignored by nextest                -> error
  a capability row deleted                             -> UNMAPPED
  a capability_set member deleted from the record      -> UNRECORDED
  a recorded member that source no longer has          -> DROPPED
  a source citation pointing at the wrong line         -> error
  the whole capability_set block deleted               -> error

The source-inventory tests use fixture crates written to a temp dir. They
exercise the lexical helpers that enumerate capability declarations, but they
are not the authority for runnable tests: hosted CI supplies the compiler-
derived nextest list for that.
"""

from __future__ import annotations

import copy
import importlib.util
import re
import sys
import tempfile
from io import StringIO
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import krites_capability_evidence as EVIDENCE

_CHECK_SCRIPT = Path(__file__).resolve().parent / "check-krites-capability-matrix.py"
_TETHERS_SCRIPT = Path(__file__).resolve().parent / "krites-tethers-remaining.py"
_GATE_WORKFLOW = (
    Path(__file__).resolve().parents[1]
    / ".github"
    / "workflows"
    / "gate-attestation.yml"
)


def _load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


CHECKER = _load("check_krites_capability_matrix", _CHECK_SCRIPT)
TETHERS = _load("krites_tethers_remaining", _TETHERS_SCRIPT)
LIVE_ROWS = CHECKER.load_matrix()
LIVE_SETS = CHECKER.load_capability_sets()

_failures: list[str] = []


def check(name: str, condition: bool, detail: str = "") -> None:
    if condition:
        print(f"  ok   {name}")
    else:
        print(f"  FAIL {name}: {detail}")
        _failures.append(name)


def _rows_with(**overrides) -> list[dict]:
    """Copy the live rows, applying `{row_id: {field: value}}` overrides."""
    rows = copy.deepcopy(LIVE_ROWS)
    for row in rows:
        patch = overrides.get(row.get("id"))
        if patch:
            row.update(patch)
    return rows


def _nextest_truth(rows: list[dict] | None = None) -> dict[str, bool]:
    """Synthetic nextest truth for exercising the resolver contract."""
    return {
        row["gate_test"]: False
        for row in (rows or LIVE_ROWS)
        if isinstance(row.get("gate_test"), str)
        and row["gate_test"].strip().lower() not in CHECKER.GATE_TEST_UNPOINTED
    }


def _nextest_workflow_selection_errors(text: str) -> list[str]:
    errors: list[str] = []
    match = re.search(r"nextest_selection=\(\n(?P<body>.*?)\n\s*\)", text, re.DOTALL)
    if match is None:
        return ["missing nextest_selection array"]
    selectors = [
        line.strip() for line in match.group("body").splitlines() if line.strip()
    ]
    expected = [
        "--profile ci",
        "--workspace",
        "--features test-core,krites_sovereign_hnsw",
    ]
    if selectors != expected:
        errors.append(f"selection array drifted: {selectors!r}")
    command_lines = [
        line.strip()
        for line in text.splitlines()
        if line.strip().startswith(("cargo nextest list", "cargo nextest run"))
    ]
    if len(command_lines) != 2:
        errors.append(f"expected one list and one run command: {command_lines!r}")
    for line in command_lines:
        if '"${nextest_selection[@]}"' not in line:
            errors.append(f"nextest command bypasses shared selection: {line!r}")
        scrubbed = line.replace('"${nextest_selection[@]}"', "")
        if re.search(
            r"(?:^|\s)(?:-p|--package|--workspace|--features|--profile)(?:\s|=)",
            scrubbed,
        ):
            errors.append(f"nextest command carries a private selector: {line!r}")
    return errors


def test_live_matrix_is_green() -> None:
    errors = CHECKER.check_all_rows_well_formed(LIVE_ROWS)
    errors += CHECKER.check_category(
        "sysop", CHECKER.extract_sysop_variants(), LIVE_ROWS, "parse/sys/mod.rs"
    )
    errors += CHECKER.check_category(
        "datavalue", CHECKER.extract_datavalue_variants(), LIVE_ROWS, "data/value.rs"
    )
    errors += CHECKER.check_category(
        "public_api",
        CHECKER.extract_lib_public_api(),
        LIVE_ROWS,
        "lib.rs",
        allowed_bundles=CHECKER.PUBLIC_API_SOURCE_BUNDLES,
    )
    errors += CHECKER.check_category(
        "fixed_rule", CHECKER.extract_fixed_rule_names(), LIVE_ROWS, "fixed_rule/mod.rs"
    )
    errors += CHECKER.check_category(
        "storage_method", CHECKER.extract_storage_methods(), LIVE_ROWS, "storage/mod.rs"
    )
    errors += CHECKER.check_capability_sets(LIVE_SETS)
    errors += CHECKER.check_file_line_refs(LIVE_ROWS)
    gate_errors, _, pointed, unpointed = CHECKER.check_gate_tests(
        LIVE_ROWS,
        nextest_tests=_nextest_truth(),
    )
    errors += gate_errors
    check("live matrix has no structural errors", not errors, str(errors[:3]))
    check("live matrix has at least one pointed row", pointed > 0, f"pointed={pointed}")
    check(
        "pointed + unpointed accounts for every row",
        pointed + unpointed == len(LIVE_ROWS),
        f"{pointed} + {unpointed} != {len(LIVE_ROWS)}",
    )


def test_gate_test_naming_a_missing_test_fails() -> None:
    target = next(
        r["id"] for r in LIVE_ROWS if r.get("gate_test") not in (None, "none")
    )
    rows = _rows_with(**{target: {"gate_test": "krites::this::test::does::not::exist"}})
    errors, _, _, _ = CHECKER.check_gate_tests(
        rows,
        nextest_tests=_nextest_truth(),
    )
    check(
        "gate_test naming no test is an error",
        any(
            "does not resolve to a runnable, filter-matching test" in e for e in errors
        ),
        str(errors[:2]),
    )


def test_gate_test_naming_an_ignored_test_fails() -> None:
    target = next(
        r["id"] for r in LIVE_ROWS if r.get("gate_test") not in (None, "none")
    )
    pointer = next(r["gate_test"] for r in LIVE_ROWS if r["id"] == target)
    truth = _nextest_truth()
    truth[pointer] = True
    errors, _, _, _ = CHECKER.check_gate_tests(
        copy.deepcopy(LIVE_ROWS),
        nextest_tests=truth,
    )
    check(
        "gate_test naming an #[ignore]d test is an error",
        any("#[ignore]d" in e for e in errors),
        str(errors[:2]),
    )


def test_none_pointer_counts_as_unpointed_not_as_an_error() -> None:
    target = next(
        r["id"] for r in LIVE_ROWS if r.get("gate_test") not in (None, "none")
    )
    _, _, base_pointed, _ = CHECKER.check_gate_tests(copy.deepcopy(LIVE_ROWS))
    rows = _rows_with(**{target: {"gate_test": "none"}})
    errors, _, pointed, _ = CHECKER.check_gate_tests(rows)
    check('"none" is not an error', not errors, str(errors[:2]))
    check('"none" reduces the pointed count', pointed == base_pointed - 1, f"{pointed}")


def test_static_gate_pointer_check_does_not_claim_cargo_truth() -> None:
    target = next(
        r["id"] for r in LIVE_ROWS if r.get("gate_test") not in (None, "none")
    )
    rows = _rows_with(**{target: {"gate_test": "krites::syntactic::but::absent"}})
    errors, _, pointed, unpointed = CHECKER.check_gate_tests(rows)
    check(
        "static mode accepts a well-shaped declaration without claiming it exists",
        not errors and pointed + unpointed == len(rows),
        str(errors[:2]),
    )

    malformed = _rows_with(**{target: {"gate_test": "not a nextest id"}})
    errors, _, _, _ = CHECKER.check_gate_tests(malformed)
    check(
        "static mode rejects a malformed gate_test id",
        any("not a `<binary-id>::<test path>`" in error for error in errors),
        str(errors[:2]),
    )


def test_empty_authoritative_nextest_listing_rejects_every_pointer() -> None:
    errors, _, pointed, unpointed = CHECKER.check_gate_tests(
        copy.deepcopy(LIVE_ROWS),
        nextest_tests={},
    )
    expected_pointed = sum(
        isinstance(row.get("gate_test"), str)
        and row["gate_test"].strip().lower() not in CHECKER.GATE_TEST_UNPOINTED
        for row in LIVE_ROWS
    )
    check(
        "an explicit empty nextest result cannot collapse to static mode",
        pointed == 0
        and unpointed == len(LIVE_ROWS)
        and sum(
            "does not resolve to a runnable, filter-matching test" in error
            for error in errors
        )
        == expected_pointed,
        f"pointed={pointed}, unpointed={unpointed}, errors={len(errors)}",
    )


def test_deleting_a_graph_algorithm_row_fails() -> None:
    rows = [
        r for r in copy.deepcopy(LIVE_ROWS) if r.get("id") != "fixed-rule-page-rank"
    ]
    errors = CHECKER.check_category(
        "fixed_rule", CHECKER.extract_fixed_rule_names(), rows, "fixed_rule/mod.rs"
    )
    check(
        "deleting a fixed_rule row is UNMAPPED",
        any("UNMAPPED [fixed_rule] PageRank" in e for e in errors),
        str(errors[:2]),
    )


def test_one_row_cannot_absorb_a_second_capability() -> None:
    rows = [
        row
        for row in _rows_with(**{"fixed-rule-bfs": {"item": "BFS, PageRank"}})
        if row.get("id") != "fixed-rule-page-rank"
    ]
    errors = CHECKER.check_category(
        "fixed_rule", CHECKER.extract_fixed_rule_names(), rows, "fixed_rule/mod.rs"
    )
    check(
        "one row cannot absorb a second fixed-rule capability",
        any("OVERBROAD [fixed_rule]" in e for e in errors),
        str(errors[:2]),
    )


def test_intentional_public_api_bundles_have_exact_membership() -> None:
    errors = CHECKER.check_category(
        "public_api",
        CHECKER.extract_lib_public_api(),
        LIVE_ROWS,
        "lib.rs",
        allowed_bundles=CHECKER.PUBLIC_API_SOURCE_BUNDLES,
    )
    check("intentional public_api bundles are accepted", not errors, str(errors[:3]))

    missing_result = CHECKER.extract_lib_public_api()
    del missing_result["Result"]
    errors = CHECKER.check_category(
        "public_api",
        missing_result,
        LIVE_ROWS,
        "lib.rs",
        allowed_bundles=CHECKER.PUBLIC_API_SOURCE_BUNDLES,
    )
    check(
        "loss from an intentional public_api bundle is MISSING",
        any(
            "MISSING [public_api]" in error and "'Result'" in error for error in errors
        ),
        str(errors[:3]),
    )

    absorbed = [
        row
        for row in _rows_with(
            **{"api-db-open-mem": {"item": "Db::open_mem, Db::open_fjall"}}
        )
        if row.get("id") != "api-db-open-fjall"
    ]
    errors = CHECKER.check_category(
        "public_api",
        CHECKER.extract_lib_public_api(),
        absorbed,
        "lib.rs",
        allowed_bundles=CHECKER.PUBLIC_API_SOURCE_BUNDLES,
    )
    check(
        "a public_api row cannot silently absorb a sibling",
        any("OVERBROAD [public_api]" in error for error in errors),
        str(errors[:3]),
    )

    drifted = _rows_with(
        **{
            "api-fixed-rule-trait": {
                "item": "FixedRule, FixedRuleInputRelation, CallbackOp"
            }
        }
    )
    errors = CHECKER.check_category(
        "public_api",
        CHECKER.extract_lib_public_api(),
        drifted,
        "lib.rs",
        allowed_bundles=CHECKER.PUBLIC_API_SOURCE_BUNDLES,
    )
    check(
        "a recorded public_api bundle cannot drift through item prose",
        any("BUNDLE DRIFT [public_api]" in error for error in errors),
        str(errors[:3]),
    )


def test_deleting_a_storage_method_row_fails() -> None:
    rows = [
        r
        for r in copy.deepcopy(LIVE_ROWS)
        if r.get("id") != "store-tx-del-range-from-persisted"
    ]
    errors = CHECKER.check_category(
        "storage_method", CHECKER.extract_storage_methods(), rows, "storage/mod.rs"
    )
    check(
        "deleting a storage_method row is UNMAPPED",
        any("StoreTx::del_range_from_persisted" in e for e in errors),
        str(errors[:2]),
    )


def test_capability_set_drift_fails_in_both_directions() -> None:
    sets = copy.deepcopy(LIVE_SETS)
    target = next(s for s in sets if s["id"] == "scalar-functions")
    dropped = target["members"].pop()
    errors = CHECKER.check_capability_sets(sets)
    check(
        "a source member missing from the record is UNRECORDED",
        any(
            f"UNRECORDED [capability_set scalar-functions] '{dropped}'" in e
            for e in errors
        ),
        str(errors[:2]),
    )

    sets = copy.deepcopy(LIVE_SETS)
    target = next(s for s in sets if s["id"] == "aggregations")
    target["members"] = sorted([*target["members"], "AGGR_INVENTED"])
    errors = CHECKER.check_capability_sets(sets)
    check(
        "a recorded member absent from source is DROPPED",
        any(
            "DROPPED [capability_set aggregations] 'AGGR_INVENTED'" in e for e in errors
        ),
        str(errors[:2]),
    )


def test_deleting_a_whole_capability_set_fails() -> None:
    sets = [s for s in copy.deepcopy(LIVE_SETS) if s["id"] != "aggregations"]
    errors = CHECKER.check_capability_sets(sets)
    check(
        "deleting a whole set is an error",
        any("has a source derivation but no row" in e for e in errors),
        str(errors[:2]),
    )


def test_source_derived_rows_require_a_source_citation() -> None:
    rows = _rows_with(**{"api-db-run": {"source": ""}})
    errors = CHECKER.check_all_rows_well_formed(rows)
    check(
        "a source-derived row cannot delete its provenance anchor",
        any("api-db-run" in error and "field 'source'" in error for error in errors),
        str(errors[:2]),
    )


def test_call_site_measurements_are_not_row_authored_shell() -> None:
    rows = _rows_with(
        **{
            "sysop-compact": {
                "call_sites": 0,
                "call_sites_method": "grep --definitely-invalid; printf fabricated",
            }
        }
    )
    errors = CHECKER.check_call_sites_measured(rows)
    check(
        "an invalid or fabricated grep cannot authorize a zero measurement",
        any(
            "sysop-compact" in error and "measurement failed" in error
            for error in errors
        ),
        str(errors[:2]),
    )


def test_not_measured_call_sites_use_checker_owned_exceptions() -> None:
    arbitrary = _rows_with(
        **{
            "sysop-compact": {
                "call_sites": -1,
                "call_sites_method": "not measured: because this row says so",
            }
        }
    )
    arbitrary_errors = CHECKER.check_call_sites_measured(arbitrary)
    wrong_owner = _rows_with(
        **{
            "api-validity-ts": {
                "call_sites_method": (
                    "covered under api-array1; not separately re-measured"
                )
            }
        }
    )
    owner_errors = CHECKER.check_call_sites_measured(wrong_owner)
    check(
        "a measured row cannot self-authorize the not-measured sentinel",
        any("not in the checker-owned" in error for error in arbitrary_errors),
        str(arbitrary_errors[:2]),
    )
    check(
        "a covered row cannot choose an arbitrary measurement owner",
        any("reviewed measurement owner" in error for error in owner_errors),
        str(owner_errors[:2]),
    )


def test_issue_completion_requires_state_and_a_closing_pr() -> None:
    complete = TETHERS.IssueStatus(
        state="CLOSED",
        state_reason="COMPLETED",
        closing_refs=("https://github.com/forkwright/aletheia/pull/1",),
    )
    bare = TETHERS.IssueStatus(state="CLOSED", state_reason="COMPLETED")
    missing_reason = TETHERS.IssueStatus(
        state="CLOSED",
        state_reason=None,
        closing_refs=("https://github.com/forkwright/aletheia/pull/1",),
    )
    check(
        "only completed issues joined to a closing PR are resolved",
        TETHERS._issue_disposition(complete) == "resolved"
        and TETHERS._issue_disposition(bare) == "unresolved"
        and TETHERS._issue_disposition(missing_reason) == "unresolved",
    )


def test_citation_pointing_at_the_wrong_line_fails() -> None:
    rows = _rows_with(**{"api-db-run": {"source": "crates/krites/src/lib.rs:1"}})
    errors = CHECKER.check_file_line_refs(rows)
    check(
        "an in-range citation that names nothing is an error",
        any("names none of the row's item tokens" in e for e in errors),
        str(errors[:2]),
    )


def test_citation_identifier_prefix_is_not_an_anchor() -> None:
    storage = CHECKER.STORAGE_FILE.read_text(encoding="utf-8").splitlines()
    prefix_line = next(
        i for i, line in enumerate(storage, 1) if "fn range_scan_tuple" in line
    )
    rows = _rows_with(
        **{
            "store-tx-range-scan": {
                "source": f"crates/krites/src/storage/mod.rs:{prefix_line}"
            }
        }
    )
    errors = CHECKER.check_file_line_refs(rows)
    check(
        "an identifier prefix is not a source anchor",
        any("names none of the row's item tokens" in e for e in errors),
        str(errors[:2]),
    )


def test_citation_comments_and_strings_are_not_code_anchors() -> None:
    original_root = CHECKER.REPO_ROOT
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        path = root / "crates" / "krites" / "src" / "storage" / "mod.rs"
        path.parent.mkdir(parents=True)
        path.write_text(
            "/*\n"
            "fn range_scan(&self);\n"
            "*/\n"
            "// fn range_scan(&self);\n"
            'const NOTE: &str = "range_scan";\n'
            "fn range_scań(&self);\n"
        )
        CHECKER.REPO_ROOT = root
        try:
            rows = [
                {
                    "id": f"comment-or-string-{line}",
                    "category": "storage_method",
                    "item": "StoreTx::range_scan",
                    "source": f"crates/krites/src/storage/mod.rs:{line}",
                }
                for line in (2, 4, 5, 6)
            ]
            errors = CHECKER.check_file_line_refs(rows)
        finally:
            CHECKER.REPO_ROOT = original_root
    check(
        "block comments, line comments, and strings are not citation anchors",
        len([error for error in errors if "names none" in error]) == 4,
        str(errors),
    )


def test_citation_cannot_escape_the_repository() -> None:
    original_root = CHECKER.REPO_ROOT
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp) / "repo"
        (root / "crates").mkdir(parents=True)
        outside = Path(tmp) / "outside.rs"
        outside.write_text("fn range_scan(&self);\n")
        CHECKER.REPO_ROOT = root
        try:
            errors = CHECKER.check_file_line_refs(
                [
                    {
                        "id": "outside-citation",
                        "category": "storage_method",
                        "item": "StoreTx::range_scan",
                        "source": "crates/../../outside.rs:1",
                    }
                ]
            )
        finally:
            CHECKER.REPO_ROOT = original_root
    check(
        "citation paths cannot traverse outside the repository",
        any("not lexically contained" in error for error in errors),
        str(errors),
    )


def test_fixed_rule_citation_requires_the_registry_owner_path() -> None:
    original_root = CHECKER.REPO_ROOT
    original_fixed = CHECKER.FIXED_RULE_FILE
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        fixed = root / "crates" / "krites" / "src" / "fixed_rule" / "mod.rs"
        wrong = root / "crates" / "other.rs"
        fixed.parent.mkdir(parents=True)
        wrong.parent.mkdir(parents=True, exist_ok=True)
        fixed.write_text(
            "static DEFAULT_FIXED_RULES: Map = LazyLock::new(|| {\n"
            '    BTreeMap::from([("PageRank".to_string(), one)])\n'
            "});\n"
        )
        wrong.write_text("first\nPageRank\n")
        CHECKER.REPO_ROOT = root
        CHECKER.FIXED_RULE_FILE = fixed
        try:
            errors = CHECKER.check_file_line_refs(
                [
                    {
                        "id": "wrong-fixed-owner",
                        "category": "fixed_rule",
                        "item": "PageRank",
                        "source": "crates/other.rs:2",
                    }
                ]
            )
        finally:
            CHECKER.REPO_ROOT = original_root
            CHECKER.FIXED_RULE_FILE = original_fixed
    check(
        "fixed-rule citations require the registry owner path as well as its line",
        any("names none" in error for error in errors),
        str(errors),
    )


def test_commented_capability_declarations_do_not_count() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        source_dir = Path(tmp)
        (source_dir / "mod.rs").write_text("mod ops;\n")
        (source_dir / "ops.rs").write_text(
            "// define_op!(COMMENTED, 1);\n"
            "/* define_op!(BLOCK_COMMENTED, 1); */\n"
            "define_op!(LIVE, 1);\n"
        )
        original_root = CHECKER.REPO_ROOT
        CHECKER.REPO_ROOT = source_dir
        try:
            found = CHECKER._scan_macro_items(source_dir, "define_op")
        finally:
            CHECKER.REPO_ROOT = original_root
        check(
            "commented macro declarations are absent",
            set(found) == {"LIVE"},
            str(found),
        )

        fixed_rule_file = source_dir / "fixed.rs"
        fixed_rule_file.write_text(
            "static DEFAULT_FIXED_RULES: Map = LazyLock::new(|| {\n"
            "    BTreeMap::from([\n"
            '        // ("Commented".to_string(), one),\n'
            '        /* ("BlockCommented".to_string(), two), */\n'
            '        ("ToString".to_string(), three),\n'
            '        ("ToOwned".to_owned(), four),\n'
            '        ("Into".into(), five),\n'
            '        (String::from("StringFrom"), six),\n'
            "    ])\n"
            "});\n"
        )
        original_fixed_rule = CHECKER.FIXED_RULE_FILE
        CHECKER.FIXED_RULE_FILE = fixed_rule_file
        try:
            fixed_rules = CHECKER.extract_fixed_rule_names()
        finally:
            CHECKER.FIXED_RULE_FILE = original_fixed_rule
        check(
            "commented keys are absent and String conversion spelling is irrelevant",
            set(fixed_rules) == {"ToString", "ToOwned", "Into", "StringFrom"},
            str(fixed_rules),
        )

        match_file = source_dir / "match.rs"
        match_file.write_text(
            "fn get_op() {\n"
            "    Some(match name {\n"
            '        // "COMMENTED" => one,\n'
            '        /* "BLOCK_COMMENTED" => two, */\n'
            '        "LIVE" |\n'
            '        "ALSO_LIVE" => three,\n'
            "    })\n"
            "}\n"
        )
        original_root = CHECKER.REPO_ROOT
        CHECKER.REPO_ROOT = source_dir
        try:
            match_keys = CHECKER._match_arm_keys(match_file, "get_op")
        finally:
            CHECKER.REPO_ROOT = original_root
        check(
            "commented match-arm keys are absent and OR-arm keys are complete",
            set(match_keys) == {"LIVE", "ALSO_LIVE"},
            str(match_keys),
        )

        match_file.write_text(
            "fn get_op() {\n"
            "    Some(match name {\n"
            '        "LIVE" => {\n'
            '            let _ = match other { "NESTED_DECOY" => 1, _ => 2 };\n'
            "            three\n"
            "        },\n"
            "        _ => return None,\n"
            "    })\n"
            "}\n"
        )
        CHECKER.REPO_ROOT = source_dir
        try:
            nested_keys = CHECKER._match_arm_keys(match_file, "get_op")
        finally:
            CHECKER.REPO_ROOT = original_root
        check(
            "nested RHS match arms are not promoted to DSL capabilities",
            set(nested_keys) == {"LIVE"},
            str(nested_keys),
        )

        macro_dir = source_dir / "macros"
        macro_dir.mkdir()
        (macro_dir / "mod.rs").write_text("mod forms;\n")
        (macro_dir / "forms.rs").write_text(
            "define_op ! (OP_PAREN, one);\n"
            "define_op! { OP_BRACE, two }\n"
            "define_op![OP_BRACKET, three];\n"
            "define_aggr ! (AGGR_PAREN, one);\n"
            "define_aggr! { AGGR_BRACE, two }\n"
            "define_aggr![AGGR_BRACKET, three];\n"
        )
        CHECKER.REPO_ROOT = source_dir
        try:
            ops = CHECKER._scan_macro_items(macro_dir, "define_op")
            aggrs = CHECKER._scan_macro_items(macro_dir, "define_aggr")
        finally:
            CHECKER.REPO_ROOT = original_root
        check(
            "macro delimiter and whitespace variants are enumerated",
            set(ops) == {"OP_PAREN", "OP_BRACE", "OP_BRACKET"}
            and set(aggrs) == {"AGGR_PAREN", "AGGR_BRACE", "AGGR_BRACKET"},
            f"ops={ops}, aggrs={aggrs}",
        )

        (macro_dir / "nested.rs").write_text(
            "fn decoy() { define_op! { OP_NESTED, one } }\n"
        )
        (macro_dir / "mod.rs").write_text("mod forms;\nmod nested;\n")
        CHECKER.REPO_ROOT = source_dir
        try:
            try:
                CHECKER._scan_macro_items(macro_dir, "define_op")
            except ValueError as error:
                nested_macro_error = str(error)
            else:
                nested_macro_error = ""
        finally:
            CHECKER.REPO_ROOT = original_root
        check(
            "nested declaration-macro tokens fail closed",
            "is nested" in nested_macro_error,
            nested_macro_error,
        )

        fixed_rule_file.write_text(
            "static DEFAULT_FIXED_RULES: Map = LazyLock::new(|| {\n"
            '    BTreeMap::from([("PageRank".to_lowercase(), one)])\n'
            "});\n"
        )
        CHECKER.FIXED_RULE_FILE = fixed_rule_file
        try:
            try:
                CHECKER.extract_fixed_rule_names()
            except ValueError as error:
                transformed_key_error = str(error)
            else:
                transformed_key_error = ""
        finally:
            CHECKER.FIXED_RULE_FILE = original_fixed_rule
        check(
            "transformed fixed-rule key expressions fail closed",
            "identity String materialization" in transformed_key_error,
            transformed_key_error,
        )

        fixed_rule_file.write_text(
            "static DEFAULT_FIXED_RULES: Map = LazyLock::new(|| {\n"
            '    let _ = BTreeMap::from([("DECOY".to_string(), zero)]);\n'
            '    BTreeMap::from([("PageRank".to_string(), one)])\n'
            "});\n"
        )
        CHECKER.FIXED_RULE_FILE = fixed_rule_file
        try:
            try:
                CHECKER.extract_fixed_rule_names()
            except ValueError as error:
                duplicate_owner_error = str(error)
            else:
                duplicate_owner_error = ""
        finally:
            CHECKER.FIXED_RULE_FILE = original_fixed_rule
        check(
            "discarded fixed-rule inventories make the owner non-unique",
            "owner is not unique" in duplicate_owner_error,
            duplicate_owner_error,
        )

        fixed_rule_file.write_text(
            "static DEFAULT_FIXED_RULES: Map = LazyLock::new(|| {\n"
            "    BTreeMap::from({\n"
            '        let _decoy = [("DECOY".to_string(), zero)];\n'
            '        [("LIVE".to_string(), one)]\n'
            "    })\n"
            "});\n"
        )
        CHECKER.FIXED_RULE_FILE = fixed_rule_file
        try:
            try:
                CHECKER.extract_fixed_rule_names()
            except ValueError as error:
                nested_constructor_error = str(error)
            else:
                nested_constructor_error = ""
        finally:
            CHECKER.FIXED_RULE_FILE = original_fixed_rule
        check(
            "a nested array cannot replace the fixed-rule constructor argument",
            "direct array inventory" in nested_constructor_error,
            nested_constructor_error,
        )

        match_file.write_text(
            "fn get_op() {\n"
            '    let _ = || Some(match name { "DELETED" => one, _ => return None, });\n'
            "    None\n"
            "}\n"
        )
        CHECKER.REPO_ROOT = source_dir
        try:
            try:
                CHECKER._match_arm_keys(match_file, "get_op")
            except ValueError as error:
                discarded_registry_error = str(error)
            else:
                discarded_registry_error = ""
        finally:
            CHECKER.REPO_ROOT = original_root
        check(
            "a discarded match registry cannot own DSL capabilities",
            "returned tail expression" in discarded_registry_error,
            discarded_registry_error,
        )

        fixed_rule_file.write_text(
            "fn outer() {\n"
            "    static DEFAULT_FIXED_RULES: Map = LazyLock::new(|| {\n"
            '        BTreeMap::from([("DECOY".to_string(), zero)])\n'
            "    });\n"
            "}\n"
            "static DEFAULT_FIXED_RULES: Map = LazyLock::new(|| {\n"
            '    BTreeMap::from([("LIVE".to_string(), one)])\n'
            "});\n"
        )
        CHECKER.FIXED_RULE_FILE = fixed_rule_file
        try:
            top_level_fixed = CHECKER.extract_fixed_rule_names()
        finally:
            CHECKER.FIXED_RULE_FILE = original_fixed_rule
        check(
            "an inner static cannot replace the top-level fixed-rule owner",
            set(top_level_fixed) == {"LIVE"},
            str(top_level_fixed),
        )

        match_file.write_text(
            "fn outer() {\n"
            '    fn get_op() { Some(match name { "DECOY" => zero, _ => return None, }) }\n'
            "}\n"
            "fn get_op() {\n"
            '    Some(match name { "LIVE" => one, _ => return None, })\n'
            "}\n"
        )
        CHECKER.REPO_ROOT = source_dir
        try:
            top_level_keys = CHECKER._match_arm_keys(match_file, "get_op")
        finally:
            CHECKER.REPO_ROOT = original_root
        check(
            "an inner function cannot replace the top-level DSL registry owner",
            set(top_level_keys) == {"LIVE"},
            str(top_level_keys),
        )


def test_block_commented_enum_and_public_items_do_not_count() -> None:
    enum_text = (
        "const _: &str = stringify!(pub enum Example { Decoy });\n"
        "pub enum Example {\n/*\nGhost,\n*/\nNumbered = 1,\nLive,\n}\n"
    )
    variants = CHECKER.extract_enum_variants(enum_text, "Example")
    check(
        "block-commented enum variants are absent and discriminants are retained",
        set(variants) == {"Numbered", "Live"},
        str(variants),
    )
    try:
        CHECKER.extract_enum_variants("pub enum Examplé { Live }\n", "Example")
    except ValueError as error:
        unicode_owner_error = str(error)
    else:
        unicode_owner_error = ""
    check(
        "a Unicode continuation cannot truncate an enum owner name",
        "found 0" in unicode_owner_error,
        unicode_owner_error,
    )
    try:
        CHECKER.extract_enum_variants(
            "\ufeff#!/usr/bin/env rustx\n#![cfg(any())]\npub enum Example { Ghost }\n",
            "Example",
        )
    except ValueError as error:
        dead_file_error = str(error)
    else:
        dead_file_error = ""
    check(
        "BOM and shebang preprocessing cannot detach a file cfg owner",
        "found 0" in dead_file_error,
        dead_file_error,
    )

    original_lib = CHECKER.LIB_FILE
    with tempfile.TemporaryDirectory() as tmp:
        fixture = Path(tmp) / "lib.rs"
        fixture.write_text(
            "const _: &str = stringify!(pub use crate::ghost::Decoy;);\n"
            "/* pub use crate::ghost::Ghost; */\n"
            "pub use crate::live::Live;\n"
            "/* pub fn vanished() {} */\n"
            "impl Db {\n"
            "    const NOTE: &str = stringify!(pub fn deleted() {});\n"
            "    pub fn unicode_namé() {}\n"
            "    pub fn present() {}\n"
            "    const MARKER: () = (); pub fn adjacent() {}\n"
            "}\n"
            "impl Other {\n"
            "    pub fn run() {}\n"
            "}\n"
            "pub enum MultiTransactionError {\n"
            "    /* GhostError, */\n"
            "    LiveError,\n"
            "}\n"
        )
        CHECKER.LIB_FILE = fixture
        try:
            public = CHECKER.extract_lib_public_api()
        finally:
            CHECKER.LIB_FILE = original_lib
    check(
        "block-commented public items are absent",
        set(public)
        == {"Live", "Db::present", "Db::adjacent", "MultiTransactionError::LiveError"},
        str(public),
    )

    with tempfile.TemporaryDirectory() as tmp:
        fixture = Path(tmp) / "lib.rs"
        fixture.write_text(
            "pub use crate::{nested::{A, B}, C};\n"
            "pub enum MultiTransactionError { LiveError }\n"
        )
        CHECKER.LIB_FILE = fixture
        try:
            try:
                CHECKER.extract_lib_public_api()
            except ValueError as error:
                nested_use_error = str(error)
            else:
                nested_use_error = ""
        finally:
            CHECKER.LIB_FILE = original_lib
    check(
        "nested public use trees fail closed instead of partially enumerating",
        "unsupported nested/glob pub use tree" in nested_use_error,
        nested_use_error,
    )


def test_statically_impossible_cfg_capabilities_do_not_count() -> None:
    check(
        "empty any() is an impossible cfg",
        not EVIDENCE.cfg_attrs_satisfiable(["cfg(any())"])
        and not EVIDENCE.cfg_attrs_satisfiable(["/* comment */ cfg(any())"])
        and not EVIDENCE.cfg_attrs_satisfiable(["cfg(any(/* comment */))"]),
    )
    check(
        "nested cfg constants and symbolic contradictions are impossible",
        not EVIDENCE.cfg_attrs_satisfiable(["cfg(not(all()))"])
        and not EVIDENCE.cfg_attrs_satisfiable(["cfg(/* comment */ false)"])
        and not EVIDENCE.cfg_attrs_satisfiable(["cfg(not(/* comment */ true))"])
        and not EVIDENCE.cfg_attrs_satisfiable(
            ['cfg(all(feature = r"same", not(feature = "same")))']
        ),
    )
    check(
        "conflicting single-valued rustc target options are impossible",
        not EVIDENCE.cfg_attrs_satisfiable(
            ['cfg(all(target_os = "linux", target_os = "windows"))']
        ),
    )
    check(
        "an unknown feature remains in the possible-build superset",
        EVIDENCE.cfg_attrs_satisfiable(['cfg(feature = "possible")'])
        and EVIDENCE.cfg_attrs_satisfiable(
            ['cfg(/* lead */ feature /* eq */ = /* value */ "possible")']
        )
        and not EVIDENCE.cfg_attrs_satisfiable(
            [
                'cfg(all(feature = "possible", /* trailing */))',
                'cfg(not(feature = r"possible"))',
            ]
        ),
    )
    check(
        "target-family aliases agree with their keyed cfg values",
        not EVIDENCE.cfg_attrs_satisfiable(
            ['cfg(all(unix, not(target_family = "unix")))']
        )
        and not EVIDENCE.cfg_attrs_satisfiable(
            ['cfg(all(target_family = "windows", not(windows)))']
        ),
    )
    at_limit = [f'cfg(feature = "f{index}")' for index in range(EVIDENCE.MAX_CFG_ATOMS)]
    over_limit = [*at_limit, 'cfg(feature = "overflow")']
    try:
        EVIDENCE.cfg_attrs_satisfiable(over_limit)
    except ValueError as error:
        cfg_bound_error = str(error)
    else:
        cfg_bound_error = ""
    check(
        "the symbolic cfg search has a fail-closed, constant-aware atom bound",
        EVIDENCE.cfg_attrs_satisfiable(at_limit)
        and "bounded SAT limit" in cfg_bound_error
        and not EVIDENCE.cfg_attrs_satisfiable([*over_limit, "cfg(any())"]),
        cfg_bound_error,
    )

    enum_text = (
        "pub enum Example {\n"
        "    #[cfg(any())]\n"
        "    Dead,\n"
        "    # /* comment */ [cfg(any())]\n"
        "    SpacedDead,\n"
        '    #[cfg(all(feature = "x", not(feature = r"x")))]\n'
        "    Contradiction,\n"
        '    #[cfg(feature = "possible")]\n'
        "    Possible,\n"
        "    Live,\n"
        "}\n"
    )
    variants = CHECKER.extract_enum_variants(enum_text, "Example")
    check(
        "impossible enum variants are absent while possible variants remain",
        set(variants) == {"Possible", "Live"},
        str(variants),
    )

    original_lib = CHECKER.LIB_FILE
    original_fixed = CHECKER.FIXED_RULE_FILE
    original_storage = CHECKER.STORAGE_FILE
    original_root = CHECKER.REPO_ROOT
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        lib = root / "lib.rs"
        lib.write_text(
            '#![cfg(feature = "file_possible")]\n'
            "struct Marker<const N: usize>;\n"
            "macro_rules! Ty { ($($tt:tt)*) => { () }; }\n"
            "#[cfg(any())]\npub use crate::dead::Dead;\n"
            '#[cfg(feature = "possible")]\npub use crate::possible::Possible;\n'
            "impl Db {\n"
            "    #![cfg(any())]\n"
            "    pub fn inner_dead() {}\n"
            "}\n"
            "impl Db {\n"
            '    #[cfg(not(feature = "file_possible"))]\n'
            "    pub fn contradictory() {}\n"
            "    #[cfg(any())]\n"
            "    pub fn dead() {}\n"
            "    pub fn body_inner_dead() { #![cfg(any())] }\n"
            "    pub fn const_body_dead() where Marker<{1}>: Sized "
            "{ #![cfg(any())] }\n"
            "    pub fn macro_body_dead() -> Ty!{} { #![cfg(any())] loop {} }\n"
            "    pub fn never_body_dead() -> ! { #![cfg(any())] loop {} }\n"
            "    pub fn mut_never_body_dead<'a>() -> &'a mut ! "
            "{ #![cfg(any())] loop {} }\n"
            "    pub fn live() {}\n"
            "}\n"
            "pub enum MultiTransactionError {\n"
            "    #[cfg(any())]\n"
            "    DeadError,\n"
            "    LiveError,\n"
            "}\n"
        )
        CHECKER.LIB_FILE = lib
        try:
            public = CHECKER.extract_lib_public_api()
        finally:
            CHECKER.LIB_FILE = original_lib
        check(
            "public uses, methods, and variants bind their cfg owners",
            set(public)
            == {
                "Possible",
                "Db::live",
                "MultiTransactionError::LiveError",
            },
            str(public),
        )

        fixed = root / "fixed.rs"
        fixed.write_text(
            "static DEFAULT_FIXED_RULES: Map = LazyLock::new(|| {\n"
            "    BTreeMap::from([\n"
            '        #[cfg(any())] ("DEAD".to_string(), dead),\n'
            '        # /* comment */ [cfg(any())] ("SPACED_DEAD".to_string(), dead),\n'
            '        #[cfg(feature = "possible")] ("POSSIBLE".to_string(), possible),\n'
            '        ("LIVE".to_string(), live),\n'
            "    ])\n"
            "});\n"
        )
        CHECKER.FIXED_RULE_FILE = fixed
        try:
            fixed_rules = CHECKER.extract_fixed_rule_names()
        finally:
            CHECKER.FIXED_RULE_FILE = original_fixed
        check(
            "impossible fixed-rule entries are absent",
            set(fixed_rules) == {"POSSIBLE", "LIVE"},
            str(fixed_rules),
        )

        fixed.write_text(
            "#[cfg(any())]\n"
            "pub(crate) static DEFAULT_FIXED_RULES: Map = LazyLock::new(|| {\n"
            '    BTreeMap::from([("DEAD".to_string(), dead)])\n'
            "});\n"
        )
        CHECKER.FIXED_RULE_FILE = fixed
        try:
            try:
                CHECKER.extract_fixed_rule_names()
            except ValueError as error:
                dead_fixed_owner_error = str(error)
            else:
                dead_fixed_owner_error = ""
        finally:
            CHECKER.FIXED_RULE_FILE = original_fixed
        check(
            "visibility cannot detach an impossible fixed-rule owner cfg",
            "found 0" in dead_fixed_owner_error,
            dead_fixed_owner_error,
        )

        fixed.write_text(
            "pub(crate) static DEFAULT_FIXED_RULES: Map = LazyLock::new(|| {\n"
            '    áBTreeMap::from([("DEAD".to_string(), dead)])\n'
            "});\n"
        )
        CHECKER.FIXED_RULE_FILE = fixed
        try:
            try:
                CHECKER.extract_fixed_rule_names()
            except ValueError as error:
                constructor_boundary_error = str(error)
            else:
                constructor_boundary_error = ""
        finally:
            CHECKER.FIXED_RULE_FILE = original_fixed
        check(
            "a Unicode identifier suffix cannot impersonate BTreeMap",
            "0 BTreeMap::from constructors" in constructor_boundary_error,
            constructor_boundary_error,
        )

        storage = root / "storage.rs"
        storage.write_text(
            "pub trait Storage {\n"
            "    #[cfg(any())]\n"
            "    fn dead(&self);\n"
            '    #[cfg(feature = "possible")]\n'
            "    fn possible(&self);\n"
            "    fn live(&self);\n"
            "}\n"
            "pub trait StoreTx {\n"
            "    #[cfg(any())]\n"
            "    fn dead_tx(&self);\n"
            "    fn live_tx(&self);\n"
            "}\n"
        )
        CHECKER.STORAGE_FILE = storage
        try:
            methods = CHECKER.extract_storage_methods()
        finally:
            CHECKER.STORAGE_FILE = original_storage
        check(
            "impossible storage methods are absent",
            set(methods) == {"Storage::possible", "Storage::live", "StoreTx::live_tx"},
            str(methods),
        )

        macro_dir = root / "macros"
        macro_dir.mkdir()
        (macro_dir / "mod.rs").write_text(
            "#[cfg(any())]\ndefine_op!(DEAD, dead);\n"
            "# /* comment */ [cfg(any())]\ndefine_op!(SPACED_DEAD, dead);\n"
            "#[cfg(any())]\nr#define_op!(RAW_DEAD, dead);\n"
            '#[cfg(feature = "possible")]\ndefine_op!(POSSIBLE, possible);\n'
            "macro_rules! ádefine_op { ($($tt:tt)*) => {}; }\n"
            "ádefine_op!(COMBINING_SUFFIX_DECOY, decoy);\n"
            "define_op!(LIVE, live);\n"
        )
        CHECKER.REPO_ROOT = root
        try:
            macros = CHECKER._scan_macro_items(macro_dir, "define_op")
        finally:
            CHECKER.REPO_ROOT = original_root
        check(
            "impossible declaration macros are absent",
            set(macros) == {"POSSIBLE", "LIVE"},
            str(macros),
        )

        macro_source = (macro_dir / "mod.rs").read_text()
        (macro_dir / "mod.rs").write_text(
            macro_source + "#[cfg(any())]\ncrate :: define_op!(PATH_DEAD, dead);\n"
        )
        CHECKER.REPO_ROOT = root
        try:
            try:
                CHECKER._scan_macro_items(macro_dir, "define_op")
            except ValueError as error:
                qualified_macro_error = str(error)
            else:
                qualified_macro_error = ""
        finally:
            CHECKER.REPO_ROOT = original_root
        check(
            "path-qualified declaration macros fail closed at their true owner",
            "path-qualified define_op!" in qualified_macro_error,
            qualified_macro_error,
        )
        (macro_dir / "mod.rs").write_text(macro_source)

        (macro_dir / "mod.rs").write_text(macro_source + "define_op!(ASCIÍ, decoy);\n")
        CHECKER.REPO_ROOT = root
        try:
            try:
                CHECKER._scan_macro_items(macro_dir, "define_op")
            except ValueError as error:
                macro_member_boundary_error = str(error)
            else:
                macro_member_boundary_error = ""
        finally:
            CHECKER.REPO_ROOT = original_root
        check(
            "a Unicode continuation cannot truncate a declaration-macro member",
            "does not start with an uppercase capability identifier"
            in macro_member_boundary_error,
            macro_member_boundary_error,
        )
        (macro_dir / "mod.rs").write_text(macro_source)

        (macro_dir / "orphan.rs").write_text("define_op!(ORPHAN, orphan);\n")
        CHECKER.REPO_ROOT = root
        try:
            try:
                CHECKER._scan_macro_items(macro_dir, "define_op")
            except ValueError as error:
                orphan_error = str(error)
            else:
                orphan_error = ""
        finally:
            CHECKER.REPO_ROOT = original_root
        check(
            "a declaration macro in an unreachable module fails closed",
            "unreachable module file" in orphan_error,
            orphan_error,
        )

        lookup = root / "lookup.rs"
        lookup.write_text(
            "fn get_op() {\n"
            "    Some(match name {\n"
            '        #[cfg(any())] "DEAD" => dead,\n'
            '        #[cfg(feature = "possible")] "POSSIBLE" => possible,\n'
            '        "LIVE" => live,\n'
            "        _ => return None,\n"
            "    })\n"
            "}\n"
        )
        CHECKER.REPO_ROOT = root
        try:
            keys = CHECKER._match_arm_keys(lookup, "get_op")
        finally:
            CHECKER.REPO_ROOT = original_root
        check(
            "impossible DSL match arms are absent",
            set(keys) == {"POSSIBLE", "LIVE"},
            str(keys),
        )

        lookup.write_text(
            "#[cfg(any())]\n"
            "pub(crate) fn get_op() {\n"
            '    Some(match name { "DEAD" => dead, _ => return None, })\n'
            "}\n"
        )
        CHECKER.REPO_ROOT = root
        try:
            try:
                CHECKER._match_arm_keys(lookup, "get_op")
            except ValueError as error:
                dead_dsl_owner_error = str(error)
            else:
                dead_dsl_owner_error = ""
        finally:
            CHECKER.REPO_ROOT = original_root
        check(
            "visibility cannot detach an impossible DSL owner cfg",
            "found 0" in dead_dsl_owner_error,
            dead_dsl_owner_error,
        )


def test_source_inventory_binds_cfg_from_the_crate_module_root() -> None:
    original_root = CHECKER.REPO_ROOT
    original_src = CHECKER.KRITES_SRC
    original_lib = CHECKER.LIB_FILE
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        src = root / "src"
        functions = src / "data" / "functions"
        functions.mkdir(parents=True)
        (src / "lib.rs").write_text("mod data;\n")
        (src / "data" / "mod.rs").write_text("#[cfg(any())]\nmod functions;\n")
        (functions / "mod.rs").write_text("define_op!(DEAD, dead);\n")
        CHECKER.REPO_ROOT = root
        CHECKER.KRITES_SRC = src
        CHECKER.LIB_FILE = src / "lib.rs"
        CHECKER._crate_module_branches.cache_clear()
        try:
            try:
                CHECKER._scan_macro_items(functions, "define_op")
            except ValueError as error:
                macro_error = str(error)
            else:
                macro_error = ""
            try:
                CHECKER._source_branches(functions / "mod.rs")
            except ValueError as error:
                owner_error = str(error)
            else:
                owner_error = ""
        finally:
            CHECKER.REPO_ROOT = original_root
            CHECKER.KRITES_SRC = original_src
            CHECKER.LIB_FILE = original_lib
            CHECKER._crate_module_branches.cache_clear()
    check(
        "an impossible ancestor module cannot preserve declaration macros",
        "unreachable" in macro_error,
        macro_error,
    )
    check(
        "an impossible ancestor makes a direct inventory owner unreachable",
        "is unreachable from" in owner_error,
        owner_error,
    )


def test_macro_inventory_follows_logical_path_descendants() -> None:
    original_root = CHECKER.REPO_ROOT
    original_src = CHECKER.KRITES_SRC
    original_lib = CHECKER.LIB_FILE
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        src = root / "src"
        functions = src / "functions"
        functions.mkdir(parents=True)
        (src / "lib.rs").write_text("mod functions;\n")
        (functions / "mod.rs").write_text('#[path = "../../outside.rs"]\nmod moved;\n')
        (root / "outside.rs").write_text("define_op!(MOVED, moved);\n")
        CHECKER.REPO_ROOT = root
        CHECKER.KRITES_SRC = src
        CHECKER.LIB_FILE = src / "lib.rs"
        CHECKER._crate_module_branches.cache_clear()
        try:
            macros = CHECKER._scan_macro_items(functions, "define_op")
        finally:
            CHECKER.REPO_ROOT = original_root
            CHECKER.KRITES_SRC = original_src
            CHECKER.LIB_FILE = original_lib
            CHECKER._crate_module_branches.cache_clear()
    check(
        "a path-moved logical descendant remains in the capability inventory",
        set(macros) == {"MOVED"},
        str(macros),
    )


def test_macro_inventory_follows_literal_module_include() -> None:
    original_root = CHECKER.REPO_ROOT
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        macro_dir = root / "macros"
        macro_dir.mkdir()
        (macro_dir / "mod.rs").write_text('include!("generated.inc");\n')
        (macro_dir / "generated.inc").write_text("define_op!(INCLUDED, included);\n")
        CHECKER.REPO_ROOT = root
        try:
            found = CHECKER._scan_macro_items(macro_dir, "define_op")
        finally:
            CHECKER.REPO_ROOT = original_root
    check(
        "a literal module include contributes declaration macros",
        set(found) == {"INCLUDED"},
        str(found),
    )


def test_macro_inventory_rejects_unresolvable_module_include() -> None:
    original_root = CHECKER.REPO_ROOT
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        macro_dir = root / "macros"
        macro_dir.mkdir()
        module = macro_dir / "mod.rs"
        module.write_text(
            '#[cfg(any())]\ninclude!(concat!("dead", ".inc"));\n'
            'include!(concat!("generated", ".inc"));\n'
        )
        CHECKER.REPO_ROOT = root
        try:
            try:
                CHECKER._scan_macro_items(macro_dir, "define_op")
            except ValueError as error:
                include_error = str(error)
            else:
                include_error = ""
        finally:
            CHECKER.REPO_ROOT = original_root
    check(
        "computed live module includes fail closed while impossible ones are skipped",
        "include!" in include_error and "direct string literal" in include_error,
        include_error,
    )


def test_public_api_inherent_impl_where_clauses_are_enumerated() -> None:
    original = CHECKER.LIB_FILE
    with tempfile.TemporaryDirectory() as tmp:
        fixture = Path(tmp) / "lib.rs"
        fixture.write_text(
            "struct Marker<const N: usize>;\n"
            "impl Db\n"
            "where\n"
            "    Marker<{ 1 }>: Sized,\n"
            "{\n"
            "    pub fn db_where() {}\n"
            "}\n"
            "impl MultiTransaction\n"
            "where\n"
            "    MultiTransaction: Sized,\n"
            "{\n"
            "    pub fn tx_where() {}\n"
            "}\n"
            "pub enum MultiTransactionError { LiveError }\n"
        )
        CHECKER.LIB_FILE = fixture
        try:
            public = CHECKER.extract_lib_public_api()
        finally:
            CHECKER.LIB_FILE = original
    check(
        "where-qualified inherent impl methods are enumerated",
        set(public)
        == {
            "Db::db_where",
            "MultiTransaction::tx_where",
            "MultiTransactionError::LiveError",
        },
        str(public),
    )


def test_qualified_storage_methods_are_enumerated() -> None:
    original = CHECKER.STORAGE_FILE
    with tempfile.TemporaryDirectory() as tmp:
        fixture = Path(tmp) / "storage.rs"
        fixture.write_text(
            "struct Marker<const N: usize>;\n"
            "macro_rules! Ty { ($($tt:tt)*) => { () }; }\n"
            "pub trait Storage {\n"
            "    const NOTE: &str = stringify!(fn decoy());\n"
            "    async fn fetch(&self);\n"
            "    unsafe fn replace(&mut self);\n"
            "    fn const_dead(&self) where Marker<{1}>: Sized "
            "{ #![cfg(any())] }\n"
            "    fn macro_dead(&self) -> Ty!{} { #![cfg(any())] loop {} }\n"
            "    fn never_dead(&self) -> ! { #![cfg(any())] loop {} }\n"
            "    fn mut_never_dead<'a>(&self) -> &'a mut ! "
            "{ #![cfg(any())] loop {} }\n"
            "}\n"
            "pub trait StoreTx {\n"
            '    extern "C" fn flush(&self);\n'
            "}\n"
        )
        CHECKER.STORAGE_FILE = fixture
        try:
            methods = CHECKER.extract_storage_methods()
        finally:
            CHECKER.STORAGE_FILE = original
        check(
            "qualified trait methods are enumerated",
            set(methods) == {"Storage::fetch", "Storage::replace", "StoreTx::flush"},
            str(methods),
        )


def test_strip_noise_preserves_length_and_blanks_literals() -> None:
    src = (
        'let s = "mod fake;"; // mod also_fake;\n/* mod block_fake; */\nfn real() {}\n'
    )
    stripped = EVIDENCE.strip_noise(src)
    check("strip_noise preserves length", len(stripped) == len(src))
    check("strip_noise removes literal content", "fake" not in stripped, stripped)
    check("strip_noise keeps code", "fn real() {}" in stripped, stripped)
    check(
        "strip_noise preserves newlines",
        stripped.count("\n") == src.count("\n"),
        f"{stripped.count(chr(10))} != {src.count(chr(10))}",
    )
    check(
        "empty and whitespace-only modules have no leading attributes",
        EVIDENCE.leading_inner_attributes("") == []
        and EVIDENCE.leading_inner_attributes("  \n") == [],
    )


def test_nextest_listing_decodes_an_explicit_stream() -> None:
    listing = StringIO(
        '{"test-count":3,"rust-suites":{"suite":{"status":"listed",'
        '"binary-id":"fixture","testcases":{'
        '"runs":{"ignored":false,"filter-match":{"status":"matches"}},'
        '"skips":{"ignored":true,"filter-match":{"status":"matches"}},'
        '"filtered":{"ignored":false,"filter-match":{'
        '"status":"mismatch","reason":"default-filter"}}}}}}'
    )
    parsed = EVIDENCE.load_nextest_list(listing)
    check(
        "nextest listing decodes the supplied stream",
        parsed == {"fixture::runs": False, "fixture::skips": True},
        str(parsed),
    )


def test_hosted_list_and_run_share_one_nextest_selection() -> None:
    workflow = _GATE_WORKFLOW.read_text(encoding="utf-8")
    errors = _nextest_workflow_selection_errors(workflow)
    mutated = workflow.replace(
        "cargo nextest list --message-format json",
        "cargo nextest list -p krites --message-format json",
        1,
    )
    mutant_errors = _nextest_workflow_selection_errors(mutated)
    check(
        "hosted nextest listing and run consume one exact selection",
        not errors,
        str(errors),
    )
    check(
        "a package-only listing cannot diverge from the workspace run",
        any("private selector" in error for error in mutant_errors),
        str(mutant_errors),
    )


def test_nextest_listing_fails_closed_on_malformed_authority() -> None:
    malformed = {
        "missing suites": '{"test-count":0}',
        "missing count": '{"rust-suites":{}}',
        "missing suite status": (
            '{"test-count":0,"rust-suites":{"suite":{"binary-id":"fixture",'
            '"testcases":{}}}}'
        ),
        "unknown suite status": (
            '{"test-count":0,"rust-suites":{"suite":{"status":"future",'
            '"binary-id":"fixture","testcases":{}}}}'
        ),
        "skipped suite with tests": (
            '{"test-count":1,"rust-suites":{"suite":{"status":"skipped",'
            '"binary-id":"fixture","testcases":{"runs":{"ignored":false,'
            '"filter-match":{"status":"matches"}}}}}}'
        ),
        "non-boolean ignored": (
            '{"test-count":1,"rust-suites":{"suite":{"status":"listed",'
            '"binary-id":"fixture","testcases":{"runs":{"ignored":0,'
            '"filter-match":{"status":"matches"}}}}}}'
        ),
        "missing filter match": (
            '{"test-count":1,"rust-suites":{"suite":{"status":"listed",'
            '"binary-id":"fixture","testcases":{"runs":{"ignored":false}}}}}'
        ),
        "unknown filter status": (
            '{"test-count":1,"rust-suites":{"suite":{"status":"listed",'
            '"binary-id":"fixture","testcases":{"runs":{"ignored":false,'
            '"filter-match":{"status":"future"}}}}}}'
        ),
        "mismatch without reason": (
            '{"test-count":1,"rust-suites":{"suite":{"status":"listed",'
            '"binary-id":"fixture","testcases":{"runs":{"ignored":false,'
            '"filter-match":{"status":"mismatch"}}}}}}'
        ),
        "count mismatch": (
            '{"test-count":2,"rust-suites":{"suite":{"status":"listed",'
            '"binary-id":"fixture","testcases":{"runs":{"ignored":false,'
            '"filter-match":{"status":"matches"}}}}}}'
        ),
        "duplicate id": (
            '{"test-count":2,"rust-suites":{'
            '"one":{"status":"listed","binary-id":"fixture","testcases":{'
            '"runs":{"ignored":false,"filter-match":{"status":"mismatch",'
            '"reason":"default-filter"}}}},'
            '"two":{"status":"listed","binary-id":"fixture","testcases":{'
            '"runs":{"ignored":false,"filter-match":{"status":"matches"}}}}}}'
        ),
    }
    rejected: list[str] = []
    for name, payload in malformed.items():
        try:
            EVIDENCE.load_nextest_list(StringIO(payload))
        except (ValueError, TypeError):
            rejected.append(name)
    check(
        "malformed nextest authority is rejected rather than decoded as empty/runnable",
        set(rejected) == set(malformed),
        f"rejected={rejected}",
    )

    skipped = StringIO(
        '{"test-count":0,"rust-suites":{"suite":{'
        '"status":"skipped-default-filter","binary-id":"fixture",'
        '"testcases":{}}}}'
    )
    check(
        "an empty skipped suite is represented honestly",
        EVIDENCE.load_nextest_list(skipped) == {},
    )


def main() -> int:
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            print(name)
            fn()
    if _failures:
        print(f"\n{len(_failures)} check(s) failed: {_failures}", file=sys.stderr)
        return 1
    print("\nall checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
