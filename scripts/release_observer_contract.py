"""Closed workflow shapes for the read-only release observers.

Both release-contract checkers consume these values so a harmless-looking
workflow edit cannot turn either observer into a skipped, masked, or different
command.  The small jobs intentionally have no extension points.
"""

from __future__ import annotations

from typing import Any

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


def _exact(value: Any, expected: Any, label: str) -> str | None:
    if value != expected:
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
