# Releasing Aletheia

## Version scheme

Semantic Versioning. Pre-1.0, MINOR bumps may include breaking changes with documented migration. PATCH bumps are backwards-compatible.

The canonical version lives in `Cargo.toml` at `[workspace.package].version`. All crates inherit it via `version.workspace = true`.

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
4. release-please creates a git tag (`vX.Y.Z`) and GitHub Release
5. The tag triggers `.github/workflows/release.yml`:
   - Verifies proskenion's standalone theatron pins match the root workspace
   - Runs `cargo test --workspace --exclude proskenion` and optional
     feature-flag compile checks
   - Builds release binaries for the supported targets (see [Supported platforms](#supported-platforms))
   - Packages the binary with public license, security, docs, examples, and
     manifest files
   - Inspects the tarball before upload so missing required contents fail the
     release job
   - Generates SHA256 checksums per binary
   - Generates and attaches an SBOM (SPDX)
   - Uploads everything to the GitHub Release

## Substance audit (manual checklist)

Before merging the release-please PR, run the substance audit against the
security-critical and execution-critical crates. This is a required manual step —
not an automated gate. Include the audit result or a link to recorded output in
the release PR description before merging.

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

The release matrix is authoritative in `.github/workflows/release.yml`. Current targets:

| Target | Runner | Method | Artifact |
|--------|--------|--------|----------|
| `x86_64-unknown-linux-musl` | `ubuntu-latest` | `cross` (static musl) | `aletheia-linux-x86_64` |
| `aarch64-apple-darwin` | `macos-14` | Native cargo build | `aletheia-macos-aarch64` |

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
Release.

### Release artifact contract

The tarball is a binary-and-docs package, not an agent-operable development package.
It contains: `LICENSE`, `LICENSE-DOCS`, `README.md`, `SECURITY.md`, `CHANGELOG.md`,
`Cargo.toml`, `Cargo.lock`, `deny.toml`, `docs/`, `instance.example/`, and `PACKAGE-MANIFEST.txt`.
Agent-facing development surfaces (`AGENTS.md`, `CLAUDE.md`, `_llm/`, `standards/`) are
intentionally excluded — they are internal development context, not runtime artifacts.

The tarball is self-describing:

`PACKAGE-MANIFEST.txt` records the version, target triple, source commit,
feature set, provenance/SBOM asset names, and SHA256, mode, and size for each
packaged file except the manifest itself.

```bash
# Download binary and checksum (Linux x86_64)
gh release download v0.10.0 -p 'aletheia-linux-x86_64*'

# Verify
sha256sum -c aletheia-linux-x86_64.sha256
```

The release attaches multiple SBOM artifacts with distinct subjects:

| Artifact | Subject | Format | Attested |
|----------|---------|--------|----------|
| `aletheia-linux-x86_64.spdx.json` | Linux x86_64 binary | SPDX | Yes (per-binary) |
| `aletheia-linux-x86_64.cdx.json` | Linux x86_64 binary | CycloneDX | Yes (per-binary) |
| `aletheia-macos-aarch64.spdx.json` | macOS ARM64 binary | SPDX | Yes (per-binary) |
| `aletheia-macos-aarch64.cdx.json` | macOS ARM64 binary | CycloneDX | Yes (per-binary) |
| `aletheia-sbom.spdx.json` | Full source workspace (Anchore) | SPDX | Informational only |
| `bom.cdx.json` | Full source workspace (cargo-cyclonedx) | CycloneDX | Informational only |

Per-binary SBOMs are attested and scoped to the linked binary artifact. Workspace SBOMs cover all Cargo dependencies regardless of which binary they end up in.

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
- All GitHub Actions are pinned to version tags (no `@main` references)
- Anchore SBOM generated on every release
