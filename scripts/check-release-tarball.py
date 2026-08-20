#!/usr/bin/env python3
"""Validate an Aletheia release tarball and its embedded package manifest."""

from __future__ import annotations

import argparse
import hashlib
import re
import stat
import sys
import tarfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

FEATURES = "recall,embed-candle"
REQUIRED_PATHS = (
    "aletheia",
    "LICENSE",
    "LICENSE-DOCS",
    "README.md",
    "SECURITY.md",
    "CHANGELOG.md",
    "Cargo.toml",
    "Cargo.lock",
    "deny.toml",
    "docs/QUICKSTART.md",
    "docs/DEPLOYMENT.md",
    "docs/RELEASING.md",
    "docs/DISASTER-RECOVERY.md",
    "instance.example/README.md",
    "PACKAGE-MANIFEST.txt",
)
TARGET_ARTIFACTS = {
    "x86_64-unknown-linux-musl": "aletheia-linux-x86_64",
    "aarch64-apple-darwin": "aletheia-macos-aarch64",
}
ROW_RE = re.compile(
    r"^(?P<digest>[0-9a-f]{64}) "
    r"(?P<mode>[0-7]{4}) (?P<size>[0-9]+) (?P<path>\S+)$"
)


@dataclass(frozen=True)
class ManifestRow:
    digest: str
    mode: int
    size: int


def _contained_cli_file(value: str) -> Path:
    """Resolve a CLI file beneath the invocation directory, without symlink escape."""
    allowed_root = Path.cwd().resolve(strict=True)
    try:
        candidate = (allowed_root / value).resolve(strict=True)
    except OSError as exc:
        raise argparse.ArgumentTypeError(f"invalid file {value!r}: {exc}") from exc
    if allowed_root not in candidate.parents:
        raise argparse.ArgumentTypeError(
            f"{value!r} resolves outside invocation directory {allowed_root}"
        )
    if not candidate.is_file():
        raise argparse.ArgumentTypeError(f"{value!r} is not a regular file")
    return candidate


def _safe_member_name(name: str, root: str) -> str | None:
    pure = PurePosixPath(name)
    if pure.is_absolute() or ".." in pure.parts or "." in pure.parts:
        return None
    if not pure.parts or pure.parts[0] != root:
        return None
    if len(pure.parts) == 1:
        return ""
    return PurePosixPath(*pure.parts[1:]).as_posix()


def _parse_manifest(data: bytes) -> tuple[dict[str, str], dict[str, ManifestRow], list[str]]:
    errors: list[str] = []
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError as exc:
        return {}, {}, [f"PACKAGE-MANIFEST.txt: invalid UTF-8: {exc}"]

    metadata: dict[str, str] = {}
    rows: dict[str, ManifestRow] = {}
    for line_number, line in enumerate(text.splitlines(), start=1):
        match = ROW_RE.fullmatch(line)
        if match is not None:
            path = match.group("path")
            pure = PurePosixPath(path)
            if pure.is_absolute() or ".." in pure.parts or "." in pure.parts:
                errors.append(
                    f"PACKAGE-MANIFEST.txt:{line_number}: unsafe path {path!r}"
                )
                continue
            if path in rows:
                errors.append(
                    f"PACKAGE-MANIFEST.txt:{line_number}: duplicate row for {path}"
                )
                continue
            rows[path] = ManifestRow(
                digest=match.group("digest"),
                mode=int(match.group("mode"), 8),
                size=int(match.group("size")),
            )
            continue

        if "=" in line:
            key, value = line.split("=", 1)
            if key in metadata:
                errors.append(
                    f"PACKAGE-MANIFEST.txt:{line_number}: duplicate metadata {key}"
                )
            else:
                metadata[key] = value
    return metadata, rows, errors


def _read_archive(
    tarball: Path, root: str
) -> tuple[dict[str, tuple[tarfile.TarInfo, bytes]], list[str]]:
    files: dict[str, tuple[tarfile.TarInfo, bytes]] = {}
    errors: list[str] = []
    try:
        with tarfile.open(tarball, mode="r:gz") as archive:
            seen_names: set[str] = set()
            for member in archive.getmembers():
                if member.name in seen_names:
                    errors.append(f"duplicate archive member: {member.name}")
                    continue
                seen_names.add(member.name)

                pure_name = PurePosixPath(member.name)
                raw_name = member.name.rstrip("/") if member.isdir() else member.name
                if raw_name != pure_name.as_posix():
                    errors.append(
                        f"non-canonical archive member path: {member.name}"
                    )
                    continue

                relative = _safe_member_name(member.name, root)
                if relative is None:
                    errors.append(f"unsafe or unexpected archive path: {member.name}")
                    continue
                if member.isdir():
                    continue
                if not member.isfile():
                    errors.append(
                        f"unsupported archive member type for {member.name}; "
                        "links are forbidden"
                    )
                    continue
                extracted = archive.extractfile(member)
                if extracted is None:
                    errors.append(f"failed to read archive member: {member.name}")
                    continue
                if relative in files:
                    errors.append(
                        f"duplicate normalized archive member: {member.name}"
                    )
                    continue
                files[relative] = (member, extracted.read())
    except (OSError, tarfile.TarError) as exc:
        errors.append(f"failed to open {tarball}: {exc}")
    return files, errors


def check_tarball(
    tarball: Path,
    version: str,
    target: str,
    source_sha: str,
    standalone_binary: Path,
) -> list[str]:
    errors: list[str] = []
    root = f"aletheia-{version}"
    artifact = TARGET_ARTIFACTS.get(target)
    if artifact is None:
        return [f"unsupported release target: {target}"]
    if re.fullmatch(r"[0-9a-f]{40}", source_sha) is None:
        return ["expected source commit must be a 40-hex SHA"]
    if not tarball.is_file():
        return [f"missing tarball {tarball}"]
    try:
        standalone_digest = hashlib.sha256(standalone_binary.read_bytes()).hexdigest()
    except OSError as exc:
        return [f"failed to hash standalone binary {standalone_binary}: {exc}"]

    files, archive_errors = _read_archive(tarball, root)
    errors.extend(archive_errors)

    for path in REQUIRED_PATHS:
        if path not in files:
            errors.append(f"missing {root}/{path}")

    manifest_entry = files.get("PACKAGE-MANIFEST.txt")
    if manifest_entry is None:
        return errors
    metadata, rows, manifest_errors = _parse_manifest(manifest_entry[1])
    errors.extend(manifest_errors)

    expected_metadata = {
        "version": version,
        "target": target,
        "source_commit": source_sha,
        "features": FEATURES,
        "provenance_asset": f"{artifact}-{version}.provenance.intoto.jsonl",
        "cyclonedx_sbom_asset": f"{artifact}-{version}.cdx.json",
        "spdx_sbom_asset": f"{artifact}-{version}.spdx.json",
    }
    for key, expected in expected_metadata.items():
        observed = metadata.get(key)
        if observed != expected:
            errors.append(
                f"PACKAGE-MANIFEST.txt: {key}={observed!r}, expected {expected!r}"
            )
    for key in ("rustc_version", "cross_version"):
        if not metadata.get(key):
            errors.append(f"PACKAGE-MANIFEST.txt: missing {key}")

    packaged = set(files) - {"PACKAGE-MANIFEST.txt"}
    manifest_paths = set(rows)
    for path in sorted(packaged - manifest_paths):
        errors.append(f"PACKAGE-MANIFEST.txt: missing row for {path}")
    for path in sorted(manifest_paths - packaged):
        errors.append(f"PACKAGE-MANIFEST.txt: row names absent file {path}")

    for path in sorted(packaged & manifest_paths):
        member, data = files[path]
        row = rows[path]
        digest = hashlib.sha256(data).hexdigest()
        mode = stat.S_IMODE(member.mode)
        if row.digest != digest:
            errors.append(
                f"PACKAGE-MANIFEST.txt: digest mismatch for {path}: "
                f"{row.digest} != {digest}"
            )
        if row.mode != mode:
            errors.append(
                f"PACKAGE-MANIFEST.txt: mode mismatch for {path}: "
                f"{row.mode:04o} != {mode:04o}"
            )
        if row.size != len(data):
            errors.append(
                f"PACKAGE-MANIFEST.txt: size mismatch for {path}: "
                f"{row.size} != {len(data)}"
            )
    binary_entry = files.get("aletheia")
    if binary_entry is not None:
        embedded_digest = hashlib.sha256(binary_entry[1]).hexdigest()
        if embedded_digest != standalone_digest:
            errors.append(
                "packaged aletheia does not equal the standalone release binary: "
                f"{embedded_digest} != {standalone_digest}"
            )
        binary_mode = stat.S_IMODE(binary_entry[0].mode)
        if binary_mode != 0o755:
            errors.append(
                f"packaged aletheia mode is {binary_mode:04o}, expected 0755"
            )
    return errors


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tarball", type=_contained_cli_file)
    parser.add_argument("version")
    parser.add_argument("target")
    parser.add_argument("source_sha")
    parser.add_argument("standalone_binary", type=_contained_cli_file)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    errors = check_tarball(
        args.tarball,
        args.version,
        args.target,
        args.source_sha,
        args.standalone_binary,
    )
    if errors:
        for error in errors:
            print(f"release-tarball: {error}", file=sys.stderr)
        return 1
    print("release-tarball: clean")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
