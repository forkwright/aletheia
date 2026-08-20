#!/usr/bin/env python3
"""Cryptographically verify every binary attestation in a release asset set."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

PROVENANCE_TYPE = "https://slsa.dev/provenance/v1"
CYCLONEDX_TYPE = "https://cyclonedx.org/bom"
SPDX_TYPE = "https://spdx.dev/Document/v2.3"
PLATFORM_ARTIFACTS = (
    "aletheia-linux-x86_64",
    "aletheia-macos-aarch64",
)
SEMVER_TAG_RE = re.compile(
    r"^v(?P<version>[0-9]+\.[0-9]+\.[0-9]+"
    r"(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)$"
)
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
REPO_RE = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")

Runner = Callable[[list[str]], subprocess.CompletedProcess[str]]


def _run(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, check=False, capture_output=True, text=True)


def _load_json(path: Path) -> tuple[object | None, str | None]:
    try:
        with path.open(encoding="utf-8") as handle:
            return json.load(handle), None
    except (OSError, json.JSONDecodeError) as exc:
        return None, str(exc)


def _verify_one(
    *,
    binary: Path,
    bundle: Path,
    predicate_type: str,
    source_sha: str,
    source_ref: str,
    repo: str,
    expected_predicate: object | None,
    runner: Runner,
) -> list[str]:
    errors: list[str] = []
    for path in (binary, bundle):
        if not path.is_file():
            errors.append(f"missing attestation input: {path.name}")
    if errors:
        return errors

    signer = f"github.com/{repo}/.github/workflows/release.yml"
    command = [
        "gh",
        "attestation",
        "verify",
        str(binary),
        "--repo",
        repo,
        "--bundle",
        str(bundle),
        "--predicate-type",
        predicate_type,
        "--source-digest",
        source_sha,
        "--source-ref",
        source_ref,
        "--signer-workflow",
        signer,
        "--deny-self-hosted-runners",
        "--format",
        "json",
    ]
    result = runner(command)
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or "no diagnostic"
        return [f"{bundle.name}: cryptographic verification failed: {detail}"]

    try:
        verification = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        return [f"{bundle.name}: verifier emitted invalid JSON: {exc}"]
    if not isinstance(verification, list) or len(verification) != 1:
        return [f"{bundle.name}: expected exactly one verified statement"]
    try:
        statement = verification[0]["verificationResult"]["statement"]
        observed_type = statement["predicateType"]
        observed_predicate = statement["predicate"]
    except (KeyError, TypeError):
        return [f"{bundle.name}: verified statement has an unexpected schema"]
    if observed_type != predicate_type:
        errors.append(
            f"{bundle.name}: predicate type {observed_type!r}, "
            f"expected {predicate_type!r}"
        )
    if expected_predicate is not None and observed_predicate != expected_predicate:
        errors.append(
            f"{bundle.name}: signed predicate does not equal the released SBOM"
        )
    return errors


def check_attestations(
    directory: Path,
    tag: str,
    source_sha: str,
    repo: str,
    *,
    runner: Runner = _run,
) -> list[str]:
    tag_match = SEMVER_TAG_RE.fullmatch(tag)
    if tag_match is None:
        return [f"release tag must have vX.Y.Z form, received {tag!r}"]
    if SHA_RE.fullmatch(source_sha) is None:
        return ["source SHA must be exactly 40 lowercase hex characters"]
    if REPO_RE.fullmatch(repo) is None:
        return [f"repository must have owner/name form, received {repo!r}"]
    if not directory.is_dir():
        return [f"release asset directory does not exist: {directory}"]

    version = tag_match.group("version")
    source_ref = f"refs/tags/{tag}"
    errors: list[str] = []
    for artifact in PLATFORM_ARTIFACTS:
        binary = directory / f"{artifact}-{version}"
        specs = (
            (
                directory / f"{artifact}-{version}.provenance.intoto.jsonl",
                PROVENANCE_TYPE,
                None,
            ),
            (
                directory / f"{artifact}-{version}.cdx.intoto.jsonl",
                CYCLONEDX_TYPE,
                directory / f"{artifact}-{version}.cdx.json",
            ),
            (
                directory / f"{artifact}-{version}.spdx.intoto.jsonl",
                SPDX_TYPE,
                directory / f"{artifact}-{version}.spdx.json",
            ),
        )
        for bundle, predicate_type, sbom_path in specs:
            expected_predicate: object | None = None
            if sbom_path is not None:
                expected_predicate, load_error = _load_json(sbom_path)
                if load_error is not None:
                    errors.append(f"{sbom_path.name}: failed to load SBOM: {load_error}")
                    continue
            errors.extend(
                _verify_one(
                    binary=binary,
                    bundle=bundle,
                    predicate_type=predicate_type,
                    source_sha=source_sha,
                    source_ref=source_ref,
                    repo=repo,
                    expected_predicate=expected_predicate,
                    runner=runner,
                )
            )
    return errors


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--repo", required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    errors = check_attestations(
        args.directory.resolve(), args.tag, args.source_sha, args.repo
    )
    if errors:
        for error in errors:
            print(f"release-attestations: {error}", file=sys.stderr)
        return 1
    print("release-attestations: all six bundles verified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
