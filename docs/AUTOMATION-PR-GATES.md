# Automation PR Gates

Automation author identity is not a verification result. Dependabot,
release-please, and other bot-authored PRs must not receive a passing gate
status only because the PR author is trusted automation.

## Required Verification

`Gate Attestation` is a hybrid trailer-verify + CI-build fallback, on every
PR including automation PRs:

- `check-trailer` (inside the fleet-shared `hybrid-gate.yml` reusable) looks
  for a `Gate-Passed:` trailer on the PR tip commit body. It matches a bare
  `^Gate-Passed:` prefix only — it does not parse the canonical shape or bind
  a claimed tree to the tip, so this step alone is a fast-path hint, not a
  verification (aletheia#6440).
- `attestation-verify` (this repo's own job, independent of the reusable)
  re-derives the verdict for real: it requires the exact shape
  `kanon <version>[+scope:...] +stages:<list> sha:<40-hex>` that
  `kanon gate --tier full --stamp` emits, requires the attested `+stages:`
  set to cover every stage `kanon.toml`'s `[gate].stages` names, and requires
  the attested `sha:` to equal `git rev-parse <tip>^{tree}` — the tip's
  actual tree, not a claim about it. `gate` (below) fails closed on a
  present-but-invalid trailer regardless of what `hybrid-gate` reported,
  since a forged trailer can make `hybrid-gate` report success too.
- `full-gate-build` runs only when no trailer was found, re-running the exact
  stages `kanon.toml`'s `[gate].stages` attests:
  - `cargo fmt --all -- --check`
  - `cargo check --workspace --all-targets --features test-core`
  - `cargo clippy --workspace --all-targets --features test-core -- -D warnings`
  - `cargo nextest run --profile ci --workspace --features test-core`
- `gate` aggregates every job above: pass if the trailer was found AND
  independently verified (or no trailer and `full-gate-build` succeeded, or
  for waived automation); fail otherwise.

`Security` runs dependency and vulnerability checks on every PR, including
Dependabot PRs:

- `cargo deny`
- `cargo audit`
- OSV Scanner

A genuinely private fleet dependency would require `FLEET_REPO_TOKEN`; today
every forkwright git dependency in `Cargo.toml` is public and pinned to an
immutable tag, so an unauthenticated fetch already resolves it, and
`needs_fleet_repo_token: true` is standing insurance against anonymous
git-fetch rate-limit flakiness, not an authentication requirement. This
repo's own `.github/actions/fleet-git-credentials` composite action (used by
`gate-coverage-scripts`/`gate-coverage-compile-checks`) reflects that: an
absent token is a supported no-op, not an error, because Dependabot-triggered
`pull_request` runs read a separate secret store and never receive repository
secrets. `hybrid-gate.yml`'s own inline credential step (forkwright/.github,
not this repo) does not use that pattern — it `exit 1`s unconditionally on an
empty token — which makes every Dependabot PR's `full-gate-build` fail before
reaching fmt/check/clippy/nextest, on a step whose failure has nothing to do
with the dependency bump under review (aletheia#6684). Fixing the credential
step to no-op on an empty token, mirroring `fleet-git-credentials`, is a
change to `forkwright/.github`; nothing in this repo can waive it without
also weakening the trailer/build ladder for non-bot PRs.

## Dependabot Auto-Merge

Only these Dependabot classes are eligible for auto-merge:

- semantic-version patch updates
- semantic-version minor updates for direct development dependencies

They are eligible only after the real verification checks report passing:
`gate`, `cargo deny`, `cargo audit`, and OSV Scanner. Missing,
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
