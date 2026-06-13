# Releasing Aletheia

## Version scheme

Semantic Versioning. Pre-1.0, MINOR bumps may include breaking changes with documented migration. PATCH bumps are backwards-compatible.

The canonical version lives in `Cargo.toml` at `[workspace.package].version`. All crates inherit it via `version.workspace = true`.

## Automated release process

1. Merge conventional-commit-formatted PRs to `main`
2. [release-please](https://github.com/googleapis/release-please) opens a
   version-bump PR that updates `.release-please-manifest.json` and `Cargo.toml`
3. Review and merge the release PR
4. release-please creates a git tag (`vX.Y.Z`) and GitHub Release
5. The tag triggers `.github/workflows/release.yml`:
   - Verifies proskenion's standalone theatron pins match the root workspace
   - Runs the full test suite
   - Builds cross-platform binaries (4 targets)
   - Packages the binary with public license, security, docs, examples, and
     manifest files
   - Inspects the tarball before upload so missing required contents fail the
     release job
   - Generates SHA256 checksums per binary
   - Generates and attaches an SBOM (SPDX)
   - Uploads everything to the GitHub Release

## Substance audit gate

Before merging the release-please PR, run the substance audit against the
security-critical and execution-critical crates. This is a manual step -
the audit is not fast enough to run on every PR - but release time is the
right moment to verify that the tests still catch real mutations.

```bash
# Install once per machine (see CLAUDE.md § Mutation testing).
cargo install cargo-mutants

# Audit each crate. `kanon audit substance` runs three detectors:
#   1. mutation          — cargo-mutants on the crate
#   2. always_default_config — config knobs that equal their documented default
#   3. tautological_doc  — "/// Returns the X" style doc comments
kanon audit substance crates/symbolon       --json > audit-symbolon.json
kanon audit substance crates/organon        --json > audit-organon.json
kanon audit substance crates/episteme       --json > audit-episteme.json
kanon audit substance crates/krites         --json > audit-krites.json
kanon audit substance crates/nous           --json > audit-nous.json
```

Treat findings per crate:

- **Security-critical (release blocker):** any FAIL on `crates/symbolon/`
  (auth, JWT, credentials) or `crates/organon/src/sandbox/` (Landlock +
  seccomp policy). Fix before tagging.
- **Execution-critical (release blocker):** any FAIL on
  `crates/episteme/src/recall.rs` (6-factor scoring),
  `crates/episteme/src/conflict.rs` (fact-conflict resolution), or
  `crates/krites/src/fixed_rule/algos/` (graph algorithms). Fix before
  tagging.
- **Advisory (file an issue, do not block):** FAILs on other crates. The
  substance gap is real - track it - but shipping can proceed.

Skip the mutation detector for the fast-path with `--no-mutations` when
only the config scan and tautological-doc detectors are needed.

## Supported platforms

| Target | Runner | Method |
|--------|--------|--------|
| `x86_64-unknown-linux-gnu` | `ubuntu-latest` | Native cargo build |
| `aarch64-unknown-linux-gnu` | `ubuntu-latest` | `cross` (Docker) |
| `x86_64-apple-darwin` | `macos-latest` | Native cross-compile |
| `aarch64-apple-darwin` | `macos-latest` | Native cargo build |

## Manual release

When release-please fails or you need an out-of-band release:

```bash
# Bump the version
scripts/bump-version.sh 0.11.0

# Commit and tag
git add -A
git commit -m "chore: release 0.11.0"
git tag v0.11.0
git push origin main --tags
```

The tag push triggers the release workflow.

## Hotfix process

```bash
# Branch from the release tag
git checkout -b hotfix/0.10.1 v0.10.0

# Apply fix, commit
git commit -m "fix(scope): description"

# Tag and push
git tag v0.10.1
git push origin hotfix/0.10.1 --tags
```

The tag push builds binaries the same way. Merge the hotfix branch back to `main` afterwards.

## Binary verification

Each binary and tarball has a `.sha256` companion file attached to the GitHub
Release. The tarball is self-describing: it includes `LICENSE`, `LICENSE-DOCS`,
`README.md`, `SECURITY.md`, `CHANGELOG.md`, `Cargo.toml`, `Cargo.lock`,
`deny.toml`, `docs/`, `instance.example/`, and `PACKAGE-MANIFEST.txt`.

`PACKAGE-MANIFEST.txt` records the version, target triple, source commit,
feature set, provenance/SBOM asset names, and SHA256, mode, and size for each
packaged file except the manifest itself.

```bash
# Download binary and checksum
gh release download v0.10.0 -p 'aletheia-linux-amd64*'

# Verify
sha256sum -c aletheia-linux-amd64.sha256
```

The SBOM (`aletheia-sbom.spdx.json`) is also attached to each release, listing all Cargo dependencies with versions.

## Supply chain

- `cargo-audit` and `cargo-deny` run on every PR (`.github/workflows/security.yml`)
- `deny.toml` enforces license policy and advisory checks
- `Cargo.lock` is committed and pinned
- All GitHub Actions are pinned to version tags (no `@main` references)
- Anchore SBOM generated on every release
