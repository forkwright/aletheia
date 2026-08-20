# Releasing Aletheia

## Version scheme

Semantic Versioning. Pre-1.0, MINOR bumps may include breaking changes with documented migration. PATCH bumps are backwards-compatible.

The canonical version lives in `Cargo.toml` at `[workspace.package].version`. All crates inherit it via `version.workspace = true`.
`release-please-config.json`, `.release-please-manifest.json`, and
`scripts/bump-version.sh` all treat that root workspace package version as the
only release version owner.

Run the release-versioning guard before changing manifests or release tooling:

```bash
scripts/check-release-versioning.py
```

The guard fails if a workspace member declares an accidental hardcoded package
version, if release-please stops updating `[workspace.package].version`, or if
the manual bump path no longer updates every declared version owner. A crate that
must be versioned separately needs an explicit release manifest entry with the
rationale, release owner, and CI enforcement before it can opt out of workspace
version inheritance.

## Cargo publish policy

Aletheia ships binaries and release archives, not crates.io packages. Every
in-repo Rust package must set `publish = false` unless a release plan first
adds public package metadata, dependency publishability, README/docs, and a
semver policy for that crate.

Run the guard before changing manifests:

```bash
scripts/check-cargo-publish-policy.py
```

## Automated release process

1. Merge conventional-commit-formatted PRs to `main`
2. [release-please](https://github.com/googleapis/release-please) opens a
   version-bump PR that updates `.release-please-manifest.json` and `Cargo.toml`
3. Review and merge the release PR after the normal PR gates pass. Release
   automation does not receive a gate waiver by author identity.
4. Release Please creates an exact `vX.Y.Z` tag and a **draft** GitHub
   Release. Its repository-token tag does not trigger a tag-push workflow, so
   `.github/workflows/release-please.yml` explicitly dispatches
   `.github/workflows/release.yml` at that tag with the emitted tag and SHA.
5. The release workflow:
   - Rejects any tag, Cargo version, Release Please manifest, checkout, or
     caller-SHA mismatch before compilation.
   - Runs the canonical gate and security workflows, with the release commit's
     docs-only gate exemption disabled.
   - Runs release-specific tests and feature-policy compile checks.
   - Builds and packages both supported targets (see
     [Supported platforms](#supported-platforms)).
   - Stages binaries, tarballs, checksums, SBOMs, and attestation bundles as
     short-lived Actions artifacts; producer jobs never upload to the Release.
   - At one final barrier, requires the exact 20-file inventory, recomputes
     outer checksums and every tar-manifest hash/mode/size, verifies the six
     signed attestations against the binary, source SHA, and release workflow,
     and proves the signed SBOM predicates equal the released SBOM files.
   - Uploads the complete set to the draft, downloads and revalidates that
     exact set, then publishes the draft as the final fallible operation.

A direct maintainer `v*` tag push enters the same workflow. When no draft
Release exists, the workflow creates one only after identity, gate, and
security checks succeed; it then uses the same staging and publication barrier.

If an infrastructure failure interrupts the handoff after the draft and tag
exist, retry the artifact workflow from the exact tag rather than from `main`:

```bash
TAG=v0.40.0
SHA="$(git rev-list -n 1 "$TAG")"
gh workflow run release.yml --ref "$TAG" \
  -f tag_name="$TAG" \
  -f release_sha="$SHA"
```

This recovery is for transient runner or API failures against unchanged tagged
code. A deterministic failure in the tagged workflow requires a separately
reviewed abort-and-recut decision; never move the existing release tag to make
new code appear under the same version.

## Substance audit (maintainer-only release qualification)

This is not a contributor PR check and is not a branch-protection context.
It applies only to the periodic Release Please version-bump PR. Before merging
that PR, the maintainer must require a `PASS` or `PASS_WITH_ADVISORIES` receipt
for its exact head SHA in the PR body. The receipt is a procedural release
condition; GitHub does not mechanically require it for ordinary PRs.

Dispatch the trusted-main workflow with the current Release Please PR and
head:

```bash
PR=1234  # Replace with the sole open release-please--branches--main PR.
SHA="$(gh pr view "$PR" --repo forkwright/aletheia --json headRefOid --jq .headRefOid)"
gh workflow run substance-audit.yml --repo forkwright/aletheia --ref main \
  -f release_pr="$PR" \
  -f expected_sha="$SHA" \
  -f advisory_issues_json='{}'
```

`.github/workflows/substance-audit.yml` rejects a foreign, stale, non-release,
or non-current-main head before executing it. Five isolated hosted jobs build
the exact private Kanon version, scrub its source and credentials, then run a
locked baseline and mutation audit for `symbolon`, `organon`, `episteme`,
`krites`, and `nous`. Kanon and cargo-mutants binaries are never uploaded.
The exact crate features, critical paths, tool versions, timeouts, and receipt
retention live in `scripts/substance-audit-policy.toml`; do not duplicate them
in a release checklist.

The classifier reads raw cargo-mutants outcomes and a complete source scan,
not Kanon's sampled evidence text:

- A missed or timed-out mutant, or a tautological doc, under a policy critical
  path blocks the release. This includes all Symbolon source, Organon's
  sandbox, Episteme's `src/recall/` and `src/conflict.rs`, and Krites's fixed
  graph algorithms.
- `NEEDS_HUMAN`, a baseline/tool/schema/timeout failure, incomplete evidence,
  or a dirty audited tree blocks the release.
- Findings outside critical paths are advisory only after each advisory class
  names an open `forkwright/aletheia` issue. The repeated
  `always_default_config` scan is one workspace advisory, not five crate
  findings.

If the first run discovers unowned advisories, file the issue or issues, then
re-adjudicate its immutable per-crate artifacts without repeating mutation
testing:

```bash
SOURCE_RUN_ID=123456789
gh workflow run substance-audit.yml --repo forkwright/aletheia --ref main \
  -f release_pr="$PR" \
  -f expected_sha="$SHA" \
  -f source_run_id="$SOURCE_RUN_ID" \
  -f advisory_issues_json='{"mutation:nous":"https://github.com/forkwright/aletheia/issues/123"}'
```

The aggregate step rechecks the live PR head and base before replacing its
bounded receipt marker. Any Release Please regeneration or main advance makes
the old receipt stale and requires a fresh five-crate audit.

## Supported platforms

The release matrix is authoritative in `.github/workflows/release.yml`. Current targets:

| Target | Runner | Method | Artifact |
|--------|--------|--------|----------|
| `x86_64-unknown-linux-musl` | `ubuntu-latest` | `cross` (static musl) | `aletheia-linux-x86_64` |
| `aarch64-apple-darwin` | `macos-15` | Native cargo build | `aletheia-macos-aarch64` |

NOTE: musl produces a fully static binary with no glibc or runtime deps, suitable for any Linux 3.10+ regardless of distro.

## Manual release

When release-please fails or you need an out-of-band release:

```bash
# Bump the version
scripts/bump-version.sh 0.11.0

# Commit and tag
git add -A
git commit -m "chore: release 0.11.0"
git tag v0.11.0
git push origin main
git push origin refs/tags/v0.11.0
```

The tag push triggers the release workflow, which creates a draft Release when
needed and publishes only after the complete artifact contract passes.

## Hotfix process

Only branch directly from a tag that already contains this release workflow.
For an older assetless tag, first backport the current release workflows,
validators, and bump script through review; otherwise the new tag executes the
old tag's broken publication path.

```bash
# Branch from the release tag
git checkout -b hotfix/0.10.1 v0.10.0

# Apply fix, commit
git commit -m "fix(scope): description"

# Advance every version owner, commit, then push the branch and exact tag
scripts/bump-version.sh 0.10.1
git add Cargo.toml Cargo.lock crates/theatron/proskenion/Cargo.lock \
  .release-please-manifest.json
git commit -m "chore(main): release 0.10.1"
git tag v0.10.1
git push origin hotfix/0.10.1
git push origin refs/tags/v0.10.1
```

The tag push builds binaries through the same path when that path exists in the
tagged tree. Merge the hotfix branch back to `main` afterwards.

## Binary verification

Each binary and tarball has a `.sha256` companion file attached to the GitHub
Release.

### Release artifact contract

The tarball is a binary-and-docs package, not an agent-operable development package.
It contains: `LICENSE`, `LICENSE-DOCS`, `README.md`, `SECURITY.md`, `CHANGELOG.md`,
`Cargo.toml`, `Cargo.lock`, `deny.toml`, `docs/`, `instance.example/`, and `PACKAGE-MANIFEST.txt`.
Agent-facing development surfaces (`AGENTS.md`, `CLAUDE.md`, `_llm/`) are
intentionally excluded — they are internal development context, not runtime artifacts.

The tarball is self-describing:

`PACKAGE-MANIFEST.txt` records the version, target triple, source commit,
feature set, provenance/SBOM asset names, and SHA256, mode, and size for each
packaged file except the manifest itself.

```bash
# Download one exact binary and its checksum (Linux x86_64)
TAG=v0.40.0
VERSION="${TAG#v}"
ASSET="aletheia-linux-x86_64-${VERSION}"
gh release download "$TAG" -p "$ASSET" -p "${ASSET}.sha256"

# Verify
sha256sum -c "${ASSET}.sha256"
```

The release attaches multiple SBOM artifacts with distinct subjects:

| Artifact | Subject | Format | Attested |
|----------|---------|--------|----------|
| `aletheia-linux-x86_64-${VERSION}.spdx.json` | Linux x86_64 binary | SPDX | Yes (per-binary) |
| `aletheia-linux-x86_64-${VERSION}.cdx.json` | Linux x86_64 binary | CycloneDX | Yes (per-binary) |
| `aletheia-macos-aarch64-${VERSION}.spdx.json` | macOS ARM64 binary | SPDX | Yes (per-binary) |
| `aletheia-macos-aarch64-${VERSION}.cdx.json` | macOS ARM64 binary | CycloneDX | Yes (per-binary) |
| `aletheia-sbom.spdx.json` | Aletheia package dependency closure (Anchore) | SPDX | Informational only |
| `bom.cdx.json` | Aletheia package dependency closure (cargo-cyclonedx) | CycloneDX | Informational only |

Per-binary SBOMs are attested and scoped to the linked binary artifact. The
informational package SBOMs describe the main `aletheia` package's dependency
closure; individual crate CycloneDX files remain build outputs rather than
release assets.

## Supply chain

- Automation PR gate and auto-merge policy is documented in
  [AUTOMATION-PR-GATES.md](AUTOMATION-PR-GATES.md)
- `cargo-audit` and `cargo-deny` run on every PR
  (`.github/workflows/security.yml`). If private dependency credentials are
  unavailable, the checks fail closed instead of reporting a green substitute
  status.
- CodeQL runs before merge through `.github/workflows/codeql-pr.yml` when a PR
  touches Rust source, Cargo manifests or lockfile, GitHub workflows,
  `.github/codeql/`, Dependabot config, `.github/SECURITY.md`, or
  `.github/pii-patterns.txt`. The
  `codeql-pr` job is the required-check surface: it reports not applicable only
  when no CodeQL-relevant paths changed, and it does not waive dependency-bot
  permission failures as green.
- `deny.toml` enforces license policy and advisory checks
- `Cargo.lock` is committed and pinned
- All GitHub Actions are pinned to immutable commit SHAs (no `@main` references)
- Anchore SBOM generated on every release
