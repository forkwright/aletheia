#!/usr/bin/env python3
"""Derive and validate the release feature-check policy."""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_POLICY = ROOT / "scripts" / "release-feature-policy.toml"
DEFAULT_FEATURE_DOC = ROOT / "docs" / "FEATURE-FLAGS.md"
REFERENCE_DOCS = (
    ROOT / "README.md",
    ROOT / "docs" / "FEATURE-FLAGS.md",
    ROOT / "docs" / "test-tiers.md",
)

FEATURE_ROW = re.compile(
    r"^\|\s*(?P<crate>[^|]+?)\s*\|\s*`(?P<feature>[^`]+)`\s*\|"
    r"\s*(?P<default>\*\*yes\*\*|yes|no)\s*\|",
    re.IGNORECASE,
)
FEATURE_REF = re.compile(r"`([a-zA-Z0-9_.-]+)/([a-zA-Z0-9_.-]+)`")


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def workspace_packages(metadata: dict[str, Any]) -> list[dict[str, Any]]:
    members = set(metadata["workspace_members"])
    return [pkg for pkg in metadata["packages"] if pkg["id"] in members]


def package_by_name(metadata: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {pkg["name"]: pkg for pkg in workspace_packages(metadata)}


def docs_key_map(metadata: dict[str, Any]) -> dict[str, str]:
    keys: dict[str, str] = {}
    ambiguous: set[str] = set()

    for pkg in workspace_packages(metadata):
        package_name = pkg["name"]
        candidates = {package_name}
        manifest = Path(pkg["manifest_path"])
        try:
            rel_manifest = manifest.relative_to(ROOT)
        except ValueError:
            rel_manifest = manifest
        if rel_manifest.name == "Cargo.toml" and rel_manifest.parent.name:
            candidates.add(rel_manifest.parent.name)

        for candidate in candidates:
            existing = keys.get(candidate)
            if existing is None:
                keys[candidate] = package_name
            elif existing != package_name:
                ambiguous.add(candidate)

    for candidate in ambiguous:
        del keys[candidate]
    return keys


def resolve_crate(label: str, key_map: dict[str, str]) -> str | None:
    primary = re.sub(r"\*\*", "", label).strip()
    alias_match = re.search(r"\(([^)]+)\)", primary)
    names = [re.sub(r"\s*\([^)]*\)", "", primary).strip()]
    if alias_match is not None:
        names.append(alias_match.group(1).strip())

    for name in names:
        package = key_map.get(name)
        if package is not None:
            return package
    return None


def policy_exclusions(policy: dict[str, Any]) -> list[dict[str, str]]:
    return [
        {
            "crate": str(item.get("crate", "")),
            "feature": str(item.get("feature", "")),
            "category": str(item.get("category", "")),
            "reason": str(item.get("reason", "")),
        }
        for item in policy.get("feature_exclusions", [])
    ]


def no_default_recipes(policy: dict[str, Any]) -> list[dict[str, Any]]:
    return list(policy.get("no_default_recipes", []))


def exclusion_matches(exclusion: dict[str, str], crate: str, feature: str) -> bool:
    crate_matches = exclusion["crate"] in {"*", crate}
    return crate_matches and exclusion["feature"] == feature


def is_excluded(
    exclusions: list[dict[str, str]], crate: str, feature: str
) -> dict[str, str] | None:
    for exclusion in exclusions:
        if exclusion_matches(exclusion, crate, feature):
            return exclusion
    return None


def derive_feature_checks(
    metadata: dict[str, Any], policy: dict[str, Any]
) -> list[dict[str, str]]:
    exclusions = policy_exclusions(policy)
    rows: list[dict[str, str]] = []

    for pkg in sorted(workspace_packages(metadata), key=lambda item: item["name"]):
        crate = pkg["name"]
        for feature in sorted(pkg.get("features", {})):
            if is_excluded(exclusions, crate, feature) is not None:
                continue
            rows.append({"crate": crate, "feature": feature})

    return rows


def derive_no_default_recipes(
    metadata: dict[str, Any], policy: dict[str, Any]
) -> list[dict[str, str]]:
    key_map = docs_key_map(metadata)
    rows: list[dict[str, str]] = []

    for recipe in no_default_recipes(policy):
        crate = resolve_crate(str(recipe.get("crate", "")), key_map)
        features = [str(feature) for feature in recipe.get("features", [])]
        rows.append(
            {
                "name": str(recipe.get("name", "")),
                "crate": crate or str(recipe.get("crate", "")),
                "features": ",".join(features),
            }
        )

    return rows


def parse_feature_table(
    text: str, metadata: dict[str, Any]
) -> tuple[dict[str, set[str]], list[str]]:
    key_map = docs_key_map(metadata)
    documented: dict[str, set[str]] = {}
    errors: list[str] = []

    for lineno, line in enumerate(text.splitlines(), start=1):
        match = FEATURE_ROW.match(line)
        if match is None:
            continue

        package = resolve_crate(match.group("crate"), key_map)
        feature = match.group("feature")
        if package is None:
            errors.append(
                f"docs/FEATURE-FLAGS.md:{lineno}: unknown crate in feature table: "
                f"{match.group('crate').strip()}"
            )
            continue

        default_text = match.group("default").replace("*", "").lower()
        is_default = default_text == "yes"
        if is_default != (feature == "default"):
            errors.append(
                f"docs/FEATURE-FLAGS.md:{lineno}: default column for "
                f"{package}/{feature} should be {'yes' if feature == 'default' else 'no'}"
            )

        documented.setdefault(package, set()).add(feature)

    return documented, errors


def validate_feature_table(
    metadata: dict[str, Any], feature_doc: Path
) -> list[str]:
    text = feature_doc.read_text(encoding="utf-8")
    documented, errors = parse_feature_table(text, metadata)
    expected = {
        pkg["name"]: set(pkg.get("features", {}))
        for pkg in workspace_packages(metadata)
        if pkg.get("features", {})
    }

    for crate, features in sorted(expected.items()):
        missing = sorted(features - documented.get(crate, set()))
        for feature in missing:
            errors.append(
                f"docs/FEATURE-FLAGS.md: missing feature table row for "
                f"{crate}/{feature}"
            )

    for crate, features in sorted(documented.items()):
        extra = sorted(features - expected.get(crate, set()))
        for feature in extra:
            errors.append(
                f"docs/FEATURE-FLAGS.md: unknown feature table row "
                f"{crate}/{feature}"
            )

    return errors


def validate_feature_references(metadata: dict[str, Any]) -> list[str]:
    key_map = docs_key_map(metadata)
    packages = package_by_name(metadata)
    errors: list[str] = []

    for path in REFERENCE_DOCS:
        text = path.read_text(encoding="utf-8")
        rel_path = path.relative_to(ROOT)
        for lineno, line in enumerate(text.splitlines(), start=1):
            for crate_ref, feature in FEATURE_REF.findall(line):
                crate = key_map.get(crate_ref)
                if crate is None:
                    continue
                if feature not in packages[crate].get("features", {}):
                    errors.append(
                        f"{rel_path}:{lineno}: unknown documented feature "
                        f"{crate_ref}/{feature}"
                    )

    removed_feature = "local" + "-llm"
    for path in [ROOT / "README.md", *sorted((ROOT / "docs").rglob("*.md"))]:
        text = path.read_text(encoding="utf-8")
        if removed_feature in text:
            errors.append(f"{path.relative_to(ROOT)}: removed feature reference found")

    return errors


def validate_policy(metadata: dict[str, Any], policy: dict[str, Any]) -> list[str]:
    packages = package_by_name(metadata)
    key_map = docs_key_map(metadata)
    errors: list[str] = []

    for exclusion in policy_exclusions(policy):
        if not exclusion["feature"] or not exclusion["category"] or not exclusion["reason"]:
            errors.append(
                "scripts/release-feature-policy.toml: feature exclusions need "
                "feature, category, and reason"
            )
            continue

        if exclusion["crate"] == "*":
            if not any(
                exclusion["feature"] in pkg.get("features", {})
                for pkg in packages.values()
            ):
                errors.append(
                    "scripts/release-feature-policy.toml: wildcard exclusion "
                    f"matches no workspace feature: {exclusion['feature']}"
                )
            continue

        crate = resolve_crate(exclusion["crate"], key_map)
        if crate is None:
            errors.append(
                "scripts/release-feature-policy.toml: unknown exclusion crate "
                f"{exclusion['crate']}"
            )
            continue
        if exclusion["feature"] not in packages[crate].get("features", {}):
            errors.append(
                "scripts/release-feature-policy.toml: unknown exclusion feature "
                f"{crate}/{exclusion['feature']}"
            )

    for recipe in no_default_recipes(policy):
        name = str(recipe.get("name", ""))
        crate = resolve_crate(str(recipe.get("crate", "")), key_map)
        if not name:
            errors.append("scripts/release-feature-policy.toml: recipe missing name")
        if not recipe.get("reason"):
            errors.append(
                f"scripts/release-feature-policy.toml: recipe {name} missing reason"
            )
        if crate is None:
            errors.append(
                "scripts/release-feature-policy.toml: unknown recipe crate "
                f"{recipe.get('crate', '')}"
            )
            continue
        for feature in recipe.get("features", []):
            if feature not in packages[crate].get("features", {}):
                errors.append(
                    "scripts/release-feature-policy.toml: unknown recipe feature "
                    f"{crate}/{feature}"
                )

    return errors


def validate(metadata: dict[str, Any], policy: dict[str, Any], feature_doc: Path) -> list[str]:
    errors = []
    errors.extend(validate_policy(metadata, policy))
    errors.extend(validate_feature_table(metadata, feature_doc))
    errors.extend(validate_feature_references(metadata))
    return errors


def matrix_for_kind(
    kind: str, metadata: dict[str, Any], policy: dict[str, Any]
) -> dict[str, list[dict[str, str]]]:
    if kind == "feature-checks":
        return {"include": derive_feature_checks(metadata, policy)}
    if kind == "no-default-recipes":
        return {"include": derive_no_default_recipes(metadata, policy)}
    raise ValueError(f"unknown matrix kind: {kind}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--metadata",
        type=Path,
        required=True,
        help="Path to `cargo metadata --format-version 1 --no-deps` JSON.",
    )
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    parser.add_argument("--feature-doc", type=Path, default=DEFAULT_FEATURE_DOC)

    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("validate")
    matrix = subparsers.add_parser("matrix")
    matrix.add_argument(
        "--kind",
        choices=("feature-checks", "no-default-recipes"),
        required=True,
    )

    return parser.parse_args()


def main() -> int:
    args = parse_args()
    metadata = load_json(args.metadata)
    policy = load_toml(args.policy)

    if args.command == "validate":
        errors = validate(metadata, policy, args.feature_doc)
        if errors:
            print("release feature policy validation failed:", file=sys.stderr)
            for error in errors:
                print(f"  - {error}", file=sys.stderr)
            return 1

        feature_checks = derive_feature_checks(metadata, policy)
        recipes = derive_no_default_recipes(metadata, policy)
        print(
            "release feature policy valid: "
            f"{len(feature_checks)} feature checks, {len(recipes)} no-default recipes"
        )
        return 0

    if args.command == "matrix":
        matrix = matrix_for_kind(args.kind, metadata, policy)
        print(json.dumps(matrix, separators=(",", ":")))
        return 0

    raise AssertionError(f"unhandled command: {args.command}")


if __name__ == "__main__":
    raise SystemExit(main())
