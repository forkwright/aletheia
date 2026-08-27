from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "hnsw-recall-discriminator.py"
SPEC = importlib.util.spec_from_file_location("hnsw_recall_discriminator", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
hrd = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = hrd
SPEC.loader.exec_module(hrd)


def event_stream(*events: str, name: str = hrd.TARGET_EVENT_NAME) -> str:
    return "\n".join(
        json.dumps({"type": "test", "event": event, "name": name}) for event in events
    )


def sidecar_document(post_reopen: tuple[int, int], post_delete: tuple[int, int]) -> dict:
    return {
        "schema_version": 1,
        "test": hrd.TARGET_TEST_NAME,
        "phases": {
            "post-reopen": {"hits": post_reopen[0], "possible": post_reopen[1]},
            "post-delete-reopen": {"hits": post_delete[0], "possible": post_delete[1]},
        },
    }


def valid_run(
    condition: str,
    position: str,
    post_delete: tuple[int, int] = (130, 150),
) -> dict:
    ordering = "AB" if (condition, position) in (
        ("serial", "first"),
        ("concurrent", "second"),
    ) else "BA"
    return {
        "condition": condition,
        "position": position,
        "ordering": ordering,
        "instrument_state": "valid",
        "protocol": {
            "state": "valid",
            "target_status": "fail" if post_delete[0] == 0 else "pass",
        },
        "sidecar": {
            "state": "valid",
            "phases": {
                "post-reopen": {"hits": 140, "possible": 150},
                "post-delete-reopen": {
                    "hits": post_delete[0],
                    "possible": post_delete[1],
                },
            },
        },
    }


class BuildCommand(unittest.TestCase):
    def test_serial_is_exact_target_and_single_threaded(self) -> None:
        command = hrd.build_command("serial", "module")
        self.assertIn("--test-threads=1", command)
        self.assertIn(f"test(={hrd.TARGET_TEST_NAME})", command)
        self.assertIn("test-core,krites_sovereign_hnsw", command)

    def test_concurrent_keeps_default_parallelism(self) -> None:
        command = hrd.build_command("concurrent", "module")
        self.assertNotIn("--test-threads=1", command)
        self.assertIn("test(~runtime::hnsw::)", command)

    def test_command_requires_typed_nextest_json(self) -> None:
        command = hrd.build_command("serial", "module")
        self.assertEqual(command[command.index("--message-format") + 1], "libtest-json-plus")
        self.assertEqual(command[command.index("--message-format-version") + 1], "0.1")
        self.assertNotIn("--no-capture", command)

    def test_package_scope_is_all_tests(self) -> None:
        self.assertIn("all()", hrd.build_command("concurrent", "package"))


class CounterbalancedSchedule(unittest.TestCase):
    def test_two_runs_per_condition_are_ab_then_ba(self) -> None:
        schedule = hrd.counterbalanced_schedule(2)
        observed = [
            (entry["ordering"], entry["position"], entry["condition"])
            for entry in schedule
        ]
        self.assertEqual(
            observed,
            [
                ("AB", "first", "serial"),
                ("AB", "second", "concurrent"),
                ("BA", "first", "concurrent"),
                ("BA", "second", "serial"),
            ],
        )

    def test_schedule_balances_each_condition_and_position(self) -> None:
        schedule = hrd.counterbalanced_schedule(10)
        for condition in hrd.CONDITIONS:
            self.assertEqual(sum(entry["condition"] == condition for entry in schedule), 10)
            for position in hrd.POSITIONS:
                self.assertEqual(
                    sum(
                        entry["condition"] == condition and entry["position"] == position
                        for entry in schedule
                    ),
                    5,
                )

    def test_odd_cardinality_is_rejected(self) -> None:
        with self.assertRaises(ValueError):
            hrd.counterbalanced_schedule(3)


class ParseNextestEvents(unittest.TestCase):
    def test_exact_started_and_ok_is_valid(self) -> None:
        parsed = hrd.parse_nextest_events(event_stream("started", "ok"))
        self.assertEqual(parsed["state"], "valid")
        self.assertEqual(parsed["target_started"], 1)
        self.assertEqual(parsed["target_completed"], 1)
        self.assertEqual(parsed["target_status"], "pass")

    def test_duplicate_started_event_fails_cardinality(self) -> None:
        parsed = hrd.parse_nextest_events(event_stream("started", "started", "failed"))
        self.assertEqual(parsed["state"], "invalid")
        self.assertIn("exactly 1 target started", " ".join(parsed["errors"]))

    def test_similarly_named_test_does_not_satisfy_exact_target(self) -> None:
        parsed = hrd.parse_nextest_events(
            event_stream("started", "ok", name=f"prefix::{hrd.TARGET_EVENT_NAME}")
        )
        self.assertEqual(parsed["state"], "invalid")
        self.assertEqual(parsed["target_started"], 0)

    def test_event_identity_includes_nextest_package_and_binary_prefix(self) -> None:
        self.assertEqual(
            hrd.TARGET_EVENT_NAME,
            f"krites::krites${hrd.TARGET_TEST_NAME}",
        )

    def test_malformed_json_fails_closed(self) -> None:
        parsed = hrd.parse_nextest_events(event_stream("started", "ok") + "\nnot-json")
        self.assertEqual(parsed["state"], "invalid")
        self.assertIn("invalid JSON", " ".join(parsed["errors"]))

    def test_ignored_is_not_a_completed_pass_or_fail(self) -> None:
        parsed = hrd.parse_nextest_events(event_stream("started", "ignored"))
        self.assertEqual(parsed["state"], "invalid")
        self.assertIsNone(parsed["target_status"])


class ParseSidecar(unittest.TestCase):
    def parse_document(self, document: object) -> dict:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "recall.json"
            path.write_text(json.dumps(document))
            return hrd.parse_sidecar(path)

    def test_integer_exact_zero_is_valid_data(self) -> None:
        parsed = self.parse_document(sidecar_document((140, 150), (0, 150)))
        self.assertEqual(parsed["state"], "valid")
        self.assertEqual(parsed["phases"]["post-delete-reopen"]["hits"], 0)

    def test_missing_is_distinct_from_parse_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            missing = hrd.parse_sidecar(Path(directory) / "missing.json")
            malformed_path = Path(directory) / "malformed.json"
            malformed_path.write_text("{")
            malformed = hrd.parse_sidecar(malformed_path)
        self.assertEqual(missing["state"], "missing")
        self.assertEqual(malformed["state"], "parse_error")

    def test_partial_sidecar_is_incomplete(self) -> None:
        document = sidecar_document((140, 150), (0, 150))
        del document["phases"]["post-delete-reopen"]
        self.assertEqual(self.parse_document(document)["state"], "incomplete")

    def test_boolean_is_not_accepted_as_an_integer(self) -> None:
        document = sidecar_document((140, 150), (0, 150))
        document["phases"]["post-delete-reopen"]["hits"] = False
        parsed = self.parse_document(document)
        self.assertEqual(parsed["state"], "invalid")
        self.assertIn("must be integers", " ".join(parsed["errors"]))

    def test_impossible_count_is_invalid(self) -> None:
        parsed = self.parse_document(sidecar_document((151, 150), (1, 150)))
        self.assertEqual(parsed["state"], "invalid")


class Measurements(unittest.TestCase):
    def test_integer_buckets_keep_zero_distinct(self) -> None:
        self.assertEqual(hrd.measurement_class({"hits": 0, "possible": 150}), "exact_zero")
        self.assertEqual(
            hrd.measurement_class({"hits": 1, "possible": 150}), "sub_floor_nonzero"
        )
        self.assertEqual(
            hrd.measurement_class({"hits": 8, "possible": 150}), "at_or_above_floor"
        )

    def test_typed_failure_with_exact_zero_is_valid_evidence(self) -> None:
        protocol = hrd.parse_nextest_events(event_stream("started", "failed"))
        sidecar = {
            "state": "valid",
            "phases": sidecar_document((140, 150), (0, 150))["phases"],
            "errors": [],
        }
        state, errors = hrd.validate_outcome(protocol, sidecar)
        self.assertEqual(state, "valid")
        self.assertEqual(errors, [])

    def test_typed_pass_cannot_contradict_zero_sidecar(self) -> None:
        protocol = hrd.parse_nextest_events(event_stream("started", "ok"))
        sidecar = {
            "state": "valid",
            "phases": sidecar_document((140, 150), (0, 150))["phases"],
            "errors": [],
        }
        state, errors = hrd.validate_outcome(protocol, sidecar)
        self.assertEqual(state, "invalid")
        self.assertIn("contradicts", " ".join(errors))

    def test_phase_stats_are_derived_from_integer_counts(self) -> None:
        runs = [
            valid_run("serial", "first", (0, 150)),
            valid_run("serial", "second", (6, 150)),
            valid_run("serial", "first", (8, 150)),
        ]
        stats = hrd.phase_stats(runs, "post-delete-reopen")
        self.assertEqual(stats["exact_zero"], 1)
        self.assertEqual(stats["sub_floor_nonzero"], 1)
        self.assertEqual(stats["samples"], 3)


class Classify(unittest.TestCase):
    def test_no_zero(self) -> None:
        runs = [valid_run(condition, position) for condition in hrd.CONDITIONS for position in hrd.POSITIONS]
        self.assertIn("no exact-zero", hrd.classify(runs))

    def test_condition_only_across_positions(self) -> None:
        runs = [
            valid_run("serial", "first", (0, 150)),
            valid_run("serial", "second", (0, 150)),
            valid_run("concurrent", "first"),
            valid_run("concurrent", "second"),
        ]
        self.assertIn("only in serial, across both", hrd.classify(runs))

    def test_first_position_across_conditions(self) -> None:
        runs = [
            valid_run("serial", "first", (0, 150)),
            valid_run("serial", "second"),
            valid_run("concurrent", "first", (0, 150)),
            valid_run("concurrent", "second"),
        ]
        self.assertIn("only when invoked first", hrd.classify(runs))

    def test_invalid_evidence_blocks_interpretation(self) -> None:
        run = valid_run("serial", "first", (0, 150))
        run["instrument_state"] = "invalid"
        self.assertIn("instrument invalid", hrd.classify([run]))

    def test_single_block_order_across_conditions_is_visible(self) -> None:
        runs = [
            valid_run("serial", "first", (0, 150)),
            valid_run("serial", "second"),
            valid_run("concurrent", "first"),
            valid_run("concurrent", "second", (0, 150)),
        ]
        self.assertIn("only in AB blocks", hrd.classify(runs))


class ResolveOutDir(unittest.TestCase):
    def test_accepts_repo_relative_path(self) -> None:
        out = hrd.resolve_out_dir("target/hnsw-recall-discriminator")
        self.assertEqual(out, hrd.REPO_ROOT / "target" / "hnsw-recall-discriminator")

    def test_rejects_dotdot_escape(self) -> None:
        with self.assertRaises(SystemExit):
            hrd.resolve_out_dir("../outside")

    def test_rejects_absolute_path(self) -> None:
        with self.assertRaises(SystemExit):
            hrd.resolve_out_dir("/tmp/outside")

    def test_rejects_repo_root_itself(self) -> None:
        with self.assertRaises(SystemExit):
            hrd.resolve_out_dir(".")


if __name__ == "__main__":
    unittest.main()
