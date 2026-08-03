#!/usr/bin/env python3
"""Fixture-based tests for scripts/release-feature-policy.py."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
from pathlib import Path


_SCRIPT_PATH = Path(__file__).parent / "release-feature-policy.py"


def _load_policy_module() -> object:
    spec = importlib.util.spec_from_file_location("release_feature_policy", _SCRIPT_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load {_SCRIPT_PATH}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["release_feature_policy"] = module
    spec.loader.exec_module(module)
    return module


POLICY = _load_policy_module()
_FAILURES: list[str] = []


def expect(condition: bool, msg: str) -> None:
    if not condition:
        _FAILURES.append(msg)


def package(name: str, features: dict[str, list[str]], path: str) -> dict[str, object]:
    package_id = f"path+file:///fixture/{path}#{name}@0.0.0"
    return {
        "name": name,
        "id": package_id,
        "features": features,
        "manifest_path": f"/tmp/cx-4942/{path}/Cargo.toml",
    }


def fixture_metadata() -> dict[str, object]:
    packages = [
        package(
            "fixture-root",
            {
                "default": ["alpha"],
                "alpha": [],
                "beta": [],
                "online-tests": [],
                "test-full": ["online-tests"],
            },
            "crates/fixture-root",
        ),
        package(
            "fixture-memory",
            {"mneme-engine": [], "storage-fjall": [], "test-core": []},
            "crates/fixture-memory",
        ),
        package("fixture-empty", {}, "crates/fixture-empty"),
    ]
    return {
        "packages": packages,
        "workspace_members": [pkg["id"] for pkg in packages],
    }


FIXTURE_POLICY = {
    "feature_exclusions": [
        {
            "crate": "*",
            "feature": "default",
            "category": "covered-by-default-gates",
            "reason": "defaults are covered elsewhere",
        },
        {
            "crate": "*",
            "feature": "online-tests",
            "category": "network",
            "reason": "network tests run elsewhere",
        },
        {
            "crate": "*",
            "feature": "test-full",
            "category": "expensive",
            "reason": "full tests run elsewhere",
        },
    ],
    "no_default_recipes": [
        {
            "name": "fixture-headless",
            "crate": "fixture-root",
            "features": ["alpha", "beta"],
            "reason": "fixture combination",
        }
    ],
}


FRESH_DOC = """\
# Feature Flag Matrix

| Crate | Feature | Default? | Enables | Depends on |
|-------|---------|----------|---------|------------|
| **fixture-root** | `default` | **yes** | `alpha` | - |
| **fixture-root** | `alpha` | no | - | - |
| **fixture-root** | `beta` | no | - | - |
| **fixture-root** | `online-tests` | no | - | - |
| **fixture-root** | `test-full` | no | - | `online-tests` |
| **fixture-memory** | `mneme-engine` | no | - | - |
| **fixture-memory** | `storage-fjall` | no | - | - |
| **fixture-memory** | `test-core` | no | - | - |
"""


def test_derived_feature_matrix_tracks_metadata() -> None:
    rows = POLICY.derive_feature_checks(fixture_metadata(), FIXTURE_POLICY)
    pairs = {(row["crate"], row["feature"]) for row in rows}

    expect(
        ("fixture-root", "alpha") in pairs,
        "metadata feature fixture-root/alpha should be checked",
    )
    expect(
        ("fixture-root", "beta") in pairs,
        "new metadata feature fixture-root/beta should be picked up automatically",
    )
    expect(
        ("fixture-memory", "mneme-engine") in pairs,
        "memory/backend features should be checked by the same policy",
    )
    expect(
        ("fixture-root", "default") not in pairs,
        "default feature should be excluded by policy",
    )
    expect(
        ("fixture-root", "online-tests") not in pairs,
        "network feature should be excluded by policy",
    )
    expect(
        ("fixture-root", "test-full") not in pairs,
        "expensive full-test feature should be excluded by policy",
    )


def test_no_default_recipe_matrix_is_manifest_driven() -> None:
    rows = POLICY.derive_no_default_recipes(fixture_metadata(), FIXTURE_POLICY)

    expect(len(rows) == 1, f"expected one recipe row, got {rows}")
    expect(rows[0]["name"] == "fixture-headless", "recipe name should be preserved")
    expect(rows[0]["crate"] == "fixture-root", "recipe crate should be resolved")
    expect(rows[0]["features"] == "alpha,beta", "recipe features should be joined")


def test_feature_table_validation_catches_drift() -> None:
    metadata = fixture_metadata()

    with tempfile.TemporaryDirectory() as tmp_str:
        doc = Path(tmp_str) / "FEATURE-FLAGS.md"
        doc.write_text(FRESH_DOC, encoding="utf-8")
        expect(
            POLICY.validate_feature_table(metadata, doc) == [],
            "fresh docs should validate",
        )

        doc.write_text(FRESH_DOC.replace("| **fixture-root** | `beta`", "| **fixture-root** | `gamma`"), encoding="utf-8")
        errors = POLICY.validate_feature_table(metadata, doc)
        expect(
            any("missing feature table row for fixture-root/beta" in err for err in errors),
            "stale docs should report the missing Cargo feature",
        )
        expect(
            any("unknown feature table row fixture-root/gamma" in err for err in errors),
            "stale docs should report the unknown documented feature",
        )


def test_policy_validation_requires_documented_exclusion_reasons() -> None:
    broken = {
        "feature_exclusions": [
            {
                "crate": "*",
                "feature": "default",
                "category": "covered-by-default-gates",
            }
        ],
        "no_default_recipes": [],
    }

    errors = POLICY.validate_policy(fixture_metadata(), broken)
    expect(
        any("feature exclusions need" in err for err in errors),
        "policy exclusions should require reason text",
    )


def _inert_feature_metadata(crate_dir: Path) -> dict[str, object]:
    pkg = package("fixture-inert", {"inert": [], "wired": ["dep:serde"]}, "crates/fixture-inert")
    pkg["manifest_path"] = str(crate_dir / "Cargo.toml")
    return {"packages": [pkg], "workspace_members": [pkg["id"]]}


def test_no_op_feature_is_rejected_unless_gated_or_allowed() -> None:
    with tempfile.TemporaryDirectory() as tmp_str:
        crate_dir = Path(tmp_str) / "fixture-inert"
        (crate_dir / "src").mkdir(parents=True)
        source = crate_dir / "src" / "lib.rs"
        source.write_text("pub fn noop() {}\n", encoding="utf-8")
        metadata = _inert_feature_metadata(crate_dir)
        empty_policy: dict[str, object] = {}

        errors = POLICY.validate_no_op_features(metadata, empty_policy)
        expect(
            any("feature `inert` activates nothing" in err for err in errors),
            "a feature that activates nothing and has no cfg reader should be rejected",
        )
        expect(
            not any("`wired`" in err for err in errors),
            "a feature that activates a dependency should not be reported",
        )

        # A cfg reader in the crate's own source is what makes the feature real.
        source.write_text(
            '#[cfg(feature = "inert")]\npub fn gated() {}\n', encoding="utf-8"
        )
        expect(
            POLICY.validate_no_op_features(metadata, empty_policy) == [],
            "a feature read by cfg in its own crate should pass",
        )

        # An allowance is the escape hatch, and it must carry a reason.
        source.write_text("pub fn noop() {}\n", encoding="utf-8")
        allowed = {
            "no_op_allowances": [
                {
                    "crate": "*",
                    "feature": "inert",
                    "category": "uniform-test-tier",
                    "reason": "fixture",
                }
            ]
        }
        expect(
            POLICY.validate_no_op_features(metadata, allowed) == [],
            "a recorded no-op allowance should silence the finding",
        )

        undocumented = {
            "no_op_allowances": [{"crate": "*", "feature": "inert", "category": "x"}],
            "feature_exclusions": [],
            "no_default_recipes": [],
        }
        expect(
            any(
                "no-op allowances need" in err
                for err in POLICY.validate_policy(metadata, undocumented)
            ),
            "no-op allowances should require reason text",
        )


def main() -> int:
    test_derived_feature_matrix_tracks_metadata()
    test_no_default_recipe_matrix_is_manifest_driven()
    test_feature_table_validation_catches_drift()
    test_policy_validation_requires_documented_exclusion_reasons()
    test_no_op_feature_is_rejected_unless_gated_or_allowed()

    if _FAILURES:
        print(f"FAIL: {len(_FAILURES)} assertion(s) failed", file=sys.stderr)
        for failure in _FAILURES:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print("OK: all release feature policy tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
