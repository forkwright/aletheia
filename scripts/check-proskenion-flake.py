#!/usr/bin/env python3
"""Verify direnv and flake wiring stays pointed at proskenion."""

from __future__ import annotations

import logging
import re
import sys
import tomllib
from pathlib import Path


LOGGER = logging.getLogger("check-proskenion-flake")
MANIFEST = Path("crates/theatron/proskenion/Cargo.toml")
ROOT_MANIFEST = Path("Cargo.toml")
INSTALL_SCRIPT = Path("scripts/install-proskenion.sh")
STALE_TOKENS = (
    ".#desktop",
    "aletheia-desktop",
    "theatron-desktop",
)

# WHY(aletheia#7204): a flake.nix that types-checks and passes the wiring
# checks above can still supply the wrong *set* of system libraries — that
# was exactly this bug (a wgpu/Vulkan stack for a GTK3/webkit2gtk app). Cross-
# check flake.nix's declared packages against the pkg-config names
# install-proskenion.sh preflights, so a future dependency-set regression
# fails this script instead of shipping a flake that cannot link.
PKG_CONFIG_TO_NIX_ATTRS: dict[str, tuple[str, ...]] = {
    "gtk+-3.0": ("pkgs.gtk3",),
    "webkit2gtk-4.1": ("pkgs.webkitgtk_4_1",),
}
# WHY: the wgpu/Vulkan stack this bug replaced (flake.nix ~50-63 before the
# fix) — proskenion's Cargo.lock carries no wgpu/vulkan/wayland crate, so none
# of these belong in flake.nix again.
STALE_WGPU_TOKENS = (
    "vulkan-loader",
    "libxkbcommon",
    "pkgs.wayland",
)

_PKG_CONFIG_LOOP_RE = re.compile(r"for pkg in ([^;]+); do")


def load_toml(path: Path) -> dict:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def preflighted_pkg_config_names(install_script_text: str) -> list[str]:
    match = _PKG_CONFIG_LOOP_RE.search(install_script_text)
    if match is None:
        return []
    return match.group(1).split()


def gtk_webkit_errors(flake: str, install_script_text: str) -> list[str]:
    errors: list[str] = []

    for pkg_config_name in preflighted_pkg_config_names(install_script_text):
        nix_attrs = PKG_CONFIG_TO_NIX_ATTRS.get(pkg_config_name)
        if nix_attrs is None:
            errors.append(
                f"{INSTALL_SCRIPT} preflights pkg-config {pkg_config_name!r}, which "
                "PKG_CONFIG_TO_NIX_ATTRS in this script does not map to a nixpkgs "
                "attribute — add the mapping so the flake stays cross-checked"
            )
            continue
        if not any(attr in flake for attr in nix_attrs):
            errors.append(
                f"flake.nix must provide one of {nix_attrs} "
                f"({INSTALL_SCRIPT} preflights pkg-config {pkg_config_name!r})"
            )

    for token in STALE_WGPU_TOKENS:
        if token in flake:
            errors.append(
                f"flake.nix references wgpu/Vulkan token {token!r}; proskenion links "
                "GTK3/webkit2gtk (see docs/DESKTOP.md), not wgpu (aletheia#7204)"
            )

    return errors


def main() -> int:
    repo_root = Path(__file__).resolve().parents[1]
    manifest_path = repo_root / MANIFEST
    manifest = load_toml(manifest_path)
    package = manifest["package"]
    package_name = package["name"]
    package_version = package["version"]
    if isinstance(package_version, dict):
        # WHY(aletheia#4726): proskenion inherits `version.workspace = true`
        # now that it is a full root-workspace member — tomllib turns that
        # into {"workspace": True}, not a string. The version this script
        # (and flake.nix) must cross-check against lives in the root
        # manifest's [workspace.package] table instead.
        root_manifest = load_toml(repo_root / ROOT_MANIFEST)
        package_version = root_manifest["workspace"]["package"]["version"]

    flake = (repo_root / "flake.nix").read_text(encoding="utf-8")
    envrc = (repo_root / ".envrc").read_text(encoding="utf-8")
    install_script_text = (repo_root / INSTALL_SCRIPT).read_text(encoding="utf-8")
    errors: list[str] = []

    if package_name != "proskenion":
        errors.append(f"{MANIFEST}: expected package name 'proskenion', found {package_name!r}")

    if "use flake .#proskenion" not in envrc:
        errors.append(".envrc must use the proskenion flake shell")

    for token in STALE_TOKENS:
        if token in flake or token in envrc:
            errors.append(f"stale desktop flake token remains: {token}")

    required_flake_fragments = (
        "./crates/theatron/proskenion/Cargo.toml",
        "proskenionName = proskenionPackage.name;",
        # WHY not a literal "proskenionVersion = proskenionPackage.version;"
        # (aletheia#4726): proskenion's manifest now declares
        # `version.workspace = true`, so `fromTOML` hands flake.nix an
        # attrset there instead of a string — the version has to fall back
        # to the root manifest's [workspace.package] table.
        "if builtins.isAttrs proskenionPackage.version",
        "then rootManifest.workspace.package.version",
        'proskenionCargoArgs = "--manifest-path crates/theatron/proskenion/Cargo.toml -p ${proskenionName}";',
        "pname = proskenionName;",
        "version = proskenionVersion;",
    )
    for fragment in required_flake_fragments:
        if fragment not in flake:
            errors.append(f"flake.nix missing expected proskenion wiring: {fragment}")

    if package_version in flake:
        errors.append(
            f"flake.nix must derive proskenion version from {MANIFEST}, not hardcode {package_version}"
        )

    errors.extend(gtk_webkit_errors(flake, install_script_text))

    if errors:
        LOGGER.error("proskenion flake check failed:")
        for error in errors:
            LOGGER.error("  - %s", error)
        return 1

    LOGGER.info("proskenion flake wiring matches %s", MANIFEST)
    return 0


if __name__ == "__main__":
    logging.basicConfig(format="%(message)s", level=logging.INFO, stream=sys.stderr)
    raise SystemExit(main())
