"""Shared exact-release-inventory classification for release observers."""

from __future__ import annotations

import importlib.util
from pathlib import Path


def _asset_checker() -> object:
    path = Path(__file__).with_name("check-release-assets.py")
    spec = importlib.util.spec_from_file_location("check_release_assets", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load exact asset contract from {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


expected_assets = _asset_checker().expected_assets


def _asset_names(assets: list[object]) -> set[str] | None:
    names: list[str] = []
    for asset in assets:
        if not isinstance(asset, dict) or not isinstance(asset.get("name"), str):
            return None
        names.append(asset["name"])
    return set(names) if len(set(names)) == len(names) else None


def release_inventory_problem(release: dict, tag: str) -> str | None:
    """Return the exact inventory failure, keeping draft separate from publish."""
    assets = release.get("assets")
    if not isinstance(assets, list):
        return "release API returned an invalid asset inventory"
    draft = release.get("draft")
    if not isinstance(draft, bool):
        return "release API returned an invalid draft state"
    if draft:
        return f"still draft with {len(assets)} asset(s)"
    names = _asset_names(assets)
    if names is None:
        return "release API returned an invalid asset inventory"
    if not names:
        return "published with zero assets"
    try:
        expected = expected_assets(tag)
    except ValueError as exc:
        return f"cannot evaluate the exact asset contract ({exc})"
    missing = sorted(expected - names)
    unexpected = sorted(names - expected)
    if missing or unexpected:
        details = []
        if missing:
            details.append(f"missing {', '.join(missing)}")
        if unexpected:
            details.append(f"unexpected {', '.join(unexpected)}")
        return f"published inventory is not exact ({'; '.join(details)})"
    return None
