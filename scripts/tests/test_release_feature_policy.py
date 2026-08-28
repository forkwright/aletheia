"""Fixture-based tests for scripts/release-feature-policy.py."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "release-feature-policy.py"
SPEC = importlib.util.spec_from_file_location("release_feature_policy", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
POLICY = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = POLICY
SPEC.loader.exec_module(POLICY)


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


class FeatureMatrixDerivation(unittest.TestCase):
    def test_derived_feature_matrix_tracks_metadata(self) -> None:
        rows = POLICY.derive_feature_checks(fixture_metadata(), FIXTURE_POLICY)
        pairs = {(row["crate"], row["feature"]) for row in rows}

        self.assertIn(
            ("fixture-root", "alpha"),
            pairs,
            "metadata feature fixture-root/alpha should be checked",
        )
        self.assertIn(
            ("fixture-root", "beta"),
            pairs,
            "new metadata feature fixture-root/beta should be picked up automatically",
        )
        self.assertIn(
            ("fixture-memory", "mneme-engine"),
            pairs,
            "memory/backend features should be checked by the same policy",
        )
        self.assertNotIn(
            ("fixture-root", "default"),
            pairs,
            "default feature should be excluded by policy",
        )
        self.assertNotIn(
            ("fixture-root", "online-tests"),
            pairs,
            "network feature should be excluded by policy",
        )
        self.assertNotIn(
            ("fixture-root", "test-full"),
            pairs,
            "expensive full-test feature should be excluded by policy",
        )


class NoDefaultRecipes(unittest.TestCase):
    def test_no_default_recipe_matrix_is_manifest_driven(self) -> None:
        rows = POLICY.derive_no_default_recipes(fixture_metadata(), FIXTURE_POLICY)

        self.assertEqual(len(rows), 1, f"expected one recipe row, got {rows}")
        self.assertEqual(rows[0]["name"], "fixture-headless", "recipe name should be preserved")
        self.assertEqual(rows[0]["crate"], "fixture-root", "recipe crate should be resolved")
        self.assertEqual(
            rows[0]["features"], "alpha,beta", "recipe features should be joined"
        )


class FeatureTableValidation(unittest.TestCase):
    def test_feature_table_validation_catches_drift(self) -> None:
        metadata = fixture_metadata()

        with tempfile.TemporaryDirectory() as tmp_str:
            doc = Path(tmp_str) / "FEATURE-FLAGS.md"
            doc.write_text(FRESH_DOC, encoding="utf-8")
            self.assertEqual(
                POLICY.validate_feature_table(metadata, doc),
                [],
                "fresh docs should validate",
            )

            doc.write_text(
                FRESH_DOC.replace("| **fixture-root** | `beta`", "| **fixture-root** | `gamma`"),
                encoding="utf-8",
            )
            errors = POLICY.validate_feature_table(metadata, doc)
            self.assertTrue(
                any("missing feature table row for fixture-root/beta" in err for err in errors),
                "stale docs should report the missing Cargo feature",
            )
            self.assertTrue(
                any("unknown feature table row fixture-root/gamma" in err for err in errors),
                "stale docs should report the unknown documented feature",
            )


class PolicyValidation(unittest.TestCase):
    def test_policy_validation_requires_documented_exclusion_reasons(self) -> None:
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
        self.assertTrue(
            any("feature exclusions need" in err for err in errors),
            "policy exclusions should require reason text",
        )


def _inert_feature_metadata(crate_dir: Path) -> dict[str, object]:
    pkg = package("fixture-inert", {"inert": [], "wired": ["dep:serde"]}, "crates/fixture-inert")
    pkg["manifest_path"] = str(crate_dir / "Cargo.toml")
    return {"packages": [pkg], "workspace_members": [pkg["id"]]}


class NoOpFeatureValidation(unittest.TestCase):
    def test_no_op_feature_is_rejected_unless_gated_or_allowed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_str:
            crate_dir = Path(tmp_str) / "fixture-inert"
            (crate_dir / "src").mkdir(parents=True)
            source = crate_dir / "src" / "lib.rs"
            source.write_text("pub fn noop() {}\n", encoding="utf-8")
            metadata = _inert_feature_metadata(crate_dir)
            empty_policy: dict[str, object] = {}

            errors = POLICY.validate_no_op_features(metadata, empty_policy)
            self.assertTrue(
                any("feature `inert` activates nothing" in err for err in errors),
                "a feature that activates nothing and has no cfg reader should be rejected",
            )
            self.assertFalse(
                any("`wired`" in err for err in errors),
                "a feature that activates a dependency should not be reported",
            )

            # A cfg reader in the crate's own source is what makes the feature real.
            source.write_text(
                '#[cfg(feature = "inert")]\npub fn gated() {}\n', encoding="utf-8"
            )
            self.assertEqual(
                POLICY.validate_no_op_features(metadata, empty_policy),
                [],
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
            self.assertEqual(
                POLICY.validate_no_op_features(metadata, allowed),
                [],
                "a recorded no-op allowance should silence the finding",
            )

            undocumented = {
                "no_op_allowances": [{"crate": "*", "feature": "inert", "category": "x"}],
                "feature_exclusions": [],
                "no_default_recipes": [],
            }
            self.assertTrue(
                any(
                    "no-op allowances need" in err
                    for err in POLICY.validate_policy(metadata, undocumented)
                ),
                "no-op allowances should require reason text",
            )


if __name__ == "__main__":
    unittest.main()
