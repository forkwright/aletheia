"""Closed workflow shapes for the read-only release observers.

Both release-contract checkers consume these values so a harmless-looking
workflow edit cannot turn either observer into a skipped, masked, or different
command.  The small jobs intentionally have no extension points.
"""

from __future__ import annotations

import re
from typing import Any

PINNED_ACTION_REFERENCE = re.compile(
    r"^(?P<action>[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_./-]+)?)"
    r"@(?P<revision>[0-9a-f]{40})$"
)
PINNED_REVISION_PLACEHOLDER = "full-sha-pin"

CHECKOUT_ACTION = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
EXACT_RELEASE_REF = "${{ inputs.release_sha || github.sha }}"
GITHUB_REPOSITORY = "${{ github.repository }}"
OUTCOME_COMMAND = "scripts/check-release-outcome.py --attempts 6 --retry-seconds 10"
HEALTH_COMMAND = "scripts/check-release-health.py --grace-hours 12"
OUTCOME_NEEDS = (
    "release-identity",
    "canonical-gate",
    "canonical-security",
    "prepare-release",
    "test",
    "feature-policy",
    "feature-check",
    "no-default-recipes",
    "build",
    "sbom",
    "publish-release",
)


def pinned_action(uses: Any) -> str | None:
    """Return the action path only for a remote reference pinned to a full SHA."""
    if not isinstance(uses, str):
        return None
    match = PINNED_ACTION_REFERENCE.match(uses)
    return match.group("action") if match else None


def normalized_action_pins(value: Any) -> Any:
    """Erase only full-SHA pin revisions so shape comparison outlives pin bumps.

    Dependabot owns action revisions; these contracts own workflow shape. A
    trusted graph therefore binds each action's identity and its full-SHA
    pinned-ness, never the revision itself: bumping a pin preserves the shape,
    while unpinning, shortening, or retargeting an action still changes it.
    """
    if isinstance(value, dict):
        return {
            key: (
                f"{action}@{PINNED_REVISION_PLACEHOLDER}"
                if key == "uses" and (action := pinned_action(item)) is not None
                else normalized_action_pins(item)
            )
            for key, item in value.items()
        }
    if isinstance(value, list):
        return [normalized_action_pins(item) for item in value]
    return value


def _exact(value: Any, expected: Any, label: str) -> str | None:
    if normalized_action_pins(value) != normalized_action_pins(expected):
        return label
    return None


def outcome_observer_error(job: Any) -> str | None:
    """Return why the terminal observer differs from its closed schema."""
    expected = {
        "needs": list(OUTCOME_NEEDS),
        "if": "${{ always() }}",
        "runs-on": "ubuntu-latest",
        "timeout-minutes": 10,
        "permissions": {"actions": "read", "contents": "read"},
        "steps": [
            {
                "uses": CHECKOUT_ACTION,
                "with": {
                    "filter": "blob:none",
                    "persist-credentials": False,
                    "ref": EXACT_RELEASE_REF,
                },
            },
            {
                "name": "Report the release outcome",
                "env": {
                    "GH_TOKEN": "${{ secrets.GITHUB_TOKEN }}",
                    "RUN_ID": "${{ github.run_id }}",
                },
                "run": OUTCOME_COMMAND,
            },
        ],
    }
    return _exact(job, expected, "release-outcome job does not match its closed schema")


def release_health_error(workflow: Any) -> str | None:
    """Return why the bounded read-only reconciliation differs from its schema."""
    expected = {
        "name": "Release health",
        True: {
            "schedule": [{"cron": "43 6 * * *"}],
            "workflow_dispatch": None,
        },
        "permissions": {"contents": "read"},
        "jobs": {
            "audit": {
                "name": "every release tag has its exact published inventory",
                "runs-on": "ubuntu-latest",
                "timeout-minutes": 10,
                "steps": [
                    {
                        "uses": CHECKOUT_ACTION,
                        "with": {
                            "filter": "blob:none",
                            "persist-credentials": False,
                        },
                    },
                    {
                        "name": "Reconcile tags against releases",
                        "env": {
                            "GH_TOKEN": "${{ secrets.GITHUB_TOKEN }}",
                            "GH_REPO": GITHUB_REPOSITORY,
                        },
                        "run": HEALTH_COMMAND,
                    },
                ],
            }
        },
    }
    return _exact(workflow, expected, "release-health workflow does not match its closed schema")
