# Developer Security Guide

Security practices for contributors working on Aletheia. For vulnerability reporting, see the root [SECURITY.md](../SECURITY.md).

## Pre-commit Setup

Install pre-commit and gitleaks to catch secrets and PII before they enter git history:

```bash
# Install pre-commit (Python)
pip install pre-commit
# OR via Homebrew
brew install pre-commit

# Install gitleaks
brew install gitleaks
# OR download from https://github.com/gitleaks/gitleaks/releases

# Activate hooks
pre-commit install
```

The `.gitleaks.toml` config at the repo root defines custom rules for Anthropic API keys, Signal CLI passwords, phone numbers, and JWT tokens in addition to the default gitleaks ruleset.

To run a manual scan:

```bash
gitleaks detect --config .gitleaks.toml --no-git --source . -v
```

## Commit Signing

All commits to `main` should be signed. GitHub shows a "Verified" badge on signed commits.

### SSH Signing (recommended)

```bash
# Generate an SSH key if you don't have one
ssh-keygen -t ed25519 -C "your-email@example.com"

# Configure git to use SSH signing
git config --global gpg.format ssh
git config --global user.signingkey ~/.ssh/id_ed25519.pub
git config --global commit.gpgsign true

# Add the public key to GitHub:
# Settings -> SSH and GPG keys -> New SSH key -> Key type: Signing Key
```

### GPG Signing (alternative)

```bash
# Generate a GPG key
gpg --full-generate-key

# Get the key ID
gpg --list-secret-keys --keyid-format=long

# Configure git
git config --global user.signingkey YOUR_KEY_ID
git config --global commit.gpgsign true

# Add the public key to GitHub:
# Settings -> SSH and GPG keys -> New GPG key
```

Enforcement via branch protection (requiring signed commits) is deferred to SEC-07 — it needs contributor onboarding first.

## Secrets Handling

- Never commit API keys, passwords, or tokens to tracked files
- Use environment variables or `instance/config/credentials/` (gitignored) for runtime secrets
- Anthropic key: `ANTHROPIC_API_KEY` environment variable
- JWT secret: generated at runtime, never persisted in config files
- If you accidentally commit a secret, rotate it immediately and use `git filter-repo` or BFG Repo-Cleaner to scrub history

## PII Handling

- Phone numbers, names, and addresses must never appear in tracked files
- The `instance/` directory is gitignored — all personal data stays there
- Log redaction: never log full phone numbers or message content
- Test data must use synthetic identities (alice, bob, acme.corp, 192.168.1.100)
- The CI PII scanner (`.github/pii-patterns.txt`) rejects commits containing personal data patterns

## Dependency Policy

- `cargo audit` runs in CI on every PR and weekly — flags known vulnerabilities in Rust dependencies
- `cargo deny` enforces license compatibility and bans specific crates
- `npm audit` checks UI dependencies (high severity, production only)
- `pip-audit` checks memory sidecar Python dependencies
- Dependabot creates PRs for vulnerable dependencies automatically

## Branch Protection

Current recommended settings for the `main` branch:

- **Require pull request before merging** — no direct pushes to main
- **Require status checks to pass** — CI (Rust, Security workflows) must be green
- **Require review from Code Owners** — CODEOWNERS file enforces review routing
- **Require signed commits** — deferred (SEC-07), document only for now
- **Do not allow force pushes** — protect git history integrity
- **Do not allow deletions** — prevent accidental branch deletion

To enable CODEOWNERS enforcement: Repository Settings -> Branches -> Branch protection rule -> check "Require review from Code Owners".
