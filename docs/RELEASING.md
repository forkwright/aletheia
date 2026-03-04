# Releasing Aletheia

## Version Scheme

Semantic Versioning. Pre-1.0, MINOR bumps may include breaking changes with documented migration. PATCH bumps are backwards-compatible.

The canonical version lives in `Cargo.toml` at `[workspace.package].version`. All crates inherit it via `version.workspace = true`.

## Automated Release Process

1. Merge conventional-commit-formatted PRs to `main`
2. [release-please](https://github.com/googleapis/release-please) opens a
   version-bump PR that updates `.release-please-manifest.json` and `Cargo.toml`
3. Review and merge the release PR
4. release-please creates a git tag (`vX.Y.Z`) and GitHub Release
5. The tag triggers `.github/workflows/release.yml`:
   - Runs the full test suite
   - Builds cross-platform binaries (4 targets)
   - Generates SHA256 checksums per binary
   - Generates and attaches an SBOM (SPDX)
   - Uploads everything to the GitHub Release

## Supported Platforms

| Target | Runner | Method |
|--------|--------|--------|
| `x86_64-unknown-linux-gnu` | `ubuntu-latest` | Native cargo build |
| `aarch64-unknown-linux-gnu` | `ubuntu-latest` | `cross` (Docker) |
| `x86_64-apple-darwin` | `macos-latest` | Native cross-compile |
| `aarch64-apple-darwin` | `macos-latest` | Native cargo build |

## Manual Release

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

## Hotfix Process

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

## Binary Verification

Each binary has a `.sha256` companion file attached to the GitHub Release.

```bash
# Download binary and checksum
gh release download v0.10.0 -p 'aletheia-linux-amd64*'

# Verify
sha256sum -c aletheia-linux-amd64.sha256
```

The SBOM (`aletheia-sbom.spdx.json`) is also attached to each release, listing all Cargo dependencies with versions.

## Supply Chain

- `cargo-audit` and `cargo-deny` run on every PR (`.github/workflows/security.yml`)
- `deny.toml` enforces license policy and advisory checks
- `Cargo.lock` is committed and pinned
- All GitHub Actions are pinned to version tags (no `@main` references)
- Anchore SBOM generated on every release
