# Git signal — open PRs, stale branches, CI status via gh CLI
from __future__ import annotations

import asyncio
import json

from loguru import logger

from . import Signal


async def collect(config: dict) -> list[Signal]:
    git_config = config.get("signals", {}).get("git", {})
    if not git_config.get("enabled"):
        return []

    repo = git_config.get("repo", "forkwright/aletheia")
    stale_branch_days = git_config.get("stale_branch_days", 7)
    signals: list[Signal] = []

    # Open PRs
    prs = await _gh_json(["pr", "list", "--repo", repo, "--state", "open", "--json",
                          "number,title,author,updatedAt,headRefName"])
    if prs:
        for pr in prs:
            signals.append(Signal(
                source="git",
                summary=f"Open PR #{pr.get('number')}: {pr.get('title', '?')[:60]}",
                urgency=0.3,
                relevant_nous=["syn"],
                details=f"branch:{pr.get('headRefName', '?')} author:{pr.get('author', {}).get('login', '?')}",
            ))

    # Failed CI checks on open PRs
    for pr in (prs or []):
        checks_text = await _gh_text(["pr", "checks", str(pr["number"]), "--repo", repo, "--failing"])
        if checks_text and checks_text.strip():
            # --failing returns tab-separated lines: name\tstatus\ttime\turl
            failing_names = []
            for line in checks_text.strip().split("\n"):
                parts = line.split("\t")
                if parts:
                    failing_names.append(parts[0])
            if failing_names:
                names = ", ".join(failing_names[:3])
                signals.append(Signal(
                    source="git",
                    summary=f"PR #{pr['number']} CI failing: {names}",
                    urgency=0.7,
                    relevant_nous=["syn"],
                    details=f"pr:{pr['number']} failures:{len(failing_names)}",
                ))

    # Stale branches (remote, not main/develop, older than threshold)
    branches = await _gh_json(["api", f"repos/{repo}/branches", "--paginate",
                                "--jq", ".[].name"])
    # gh api returns raw JSON array; --jq gives newline-separated names
    # Fall back to listing if that doesn't parse
    if isinstance(branches, list):
        remote_branches = [b for b in branches if b not in ("main", "develop")]
    elif isinstance(branches, str):
        remote_branches = [b.strip() for b in branches.strip().split("\n")
                           if b.strip() and b.strip() not in ("main", "develop")]
    else:
        remote_branches = []

    if len(remote_branches) > 10:
        signals.append(Signal(
            source="git",
            summary=f"{len(remote_branches)} remote branches — consider cleanup",
            urgency=0.2,
            relevant_nous=["syn"],
            details=f"branches: {', '.join(remote_branches[:5])}...",
        ))

    return signals


async def _gh_text(args: list[str]) -> str | None:
    """Run gh CLI and return raw stdout text."""
    try:
        proc = await asyncio.create_subprocess_exec(
            "gh", *args,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await proc.communicate()
        if proc.returncode != 0:
            return None
        return stdout.decode().strip()
    except Exception:
        return None


async def _gh_json(args: list[str]) -> list | str | None:
    """Run gh CLI and return parsed JSON or raw stdout."""
    try:
        proc = await asyncio.create_subprocess_exec(
            "gh", *args,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await proc.communicate()
        if proc.returncode != 0:
            err = stderr.decode().strip()
            if "no checks" in err.lower() or "no failing" in err.lower():
                return []
            logger.debug(f"gh {' '.join(args[:3])} failed: {err[:100]}")
            return None
        text = stdout.decode().strip()
        if not text:
            return []
        try:
            return json.loads(text)
        except json.JSONDecodeError:
            return text
    except FileNotFoundError:
        logger.warning("gh CLI not found — git signal disabled")
        return None
    except Exception as e:
        logger.warning(f"gh command failed: {e}")
        return None
