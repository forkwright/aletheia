# Automation PR Gates

Automation author identity is not a verification result. Dependabot,
release-please, and other bot-authored PRs must not receive a passing gate
status only because the PR author is trusted automation.

## Required Verification

`Gate Attestation` runs the workspace gate on every PR, including automation
PRs:

- `cargo fmt --all -- --check`
- publish and workspace policy checks
- Poiesis feature checks
- `cargo clippy --workspace --all-targets -- -D warnings`
- fuzz target compile checks
- `cargo nextest run --profile ci --workspace --features test-core`

`Security` runs dependency and vulnerability checks on every PR, including
Dependabot PRs:

- `cargo deny`
- `cargo audit`
- OSV Scanner

Private fleet dependencies require `FLEET_REPO_TOKEN`. The token should be a
least-privilege GitHub App installation token, fine-grained token, Dependabot
secret, or equivalent mirrored public dependency path with read access only to
the required private dependency repositories. If the credential is unavailable,
the check fails closed and the PR requires maintainer verification; CI must not
emit a green substitute status for unverified code.

## Dependabot Auto-Merge

Only these Dependabot classes are eligible for auto-merge:

- semantic-version patch updates
- semantic-version minor updates for direct development dependencies

They are eligible only after the real verification checks report passing:
`gate-attestation`, `cargo deny`, `cargo audit`, and OSV Scanner. Missing,
skipped, canceled, neutral, or failed verification checks block auto-merge.

All other Dependabot updates require human review and merge. This includes
major updates, minor runtime dependency updates, updates that modify workflows or
repository policy, and any update whose dependency metadata is unavailable.

## Release-Please

Release-please PRs are not auto-merged. Version and changelog-only release PRs
may be reviewed as release metadata changes, but they still run the normal PR
verification. If a release-please PR includes source, config, workflow, or
dependency-policy changes, review it as a normal code/configuration PR.

## Regression Guard

`scripts/check-automation-pr-gates.py` is wired into
`.github/workflows/yaml-validate.yml`. It checks for the #4931 regression class:
bot-author gate pass steps, Dependabot security skips, successful exits when
private dependency credentials are missing, and Dependabot auto-merge that does
not require the real verification jobs.
