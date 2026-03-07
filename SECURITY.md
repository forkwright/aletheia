# Security

## Reporting Vulnerabilities

**Do not open a public GitHub issue for security vulnerabilities.**

Report privately via GitHub's security advisory system:
> https://github.com/forkwright/aletheia/security/advisories/new

Include: description, reproduction steps, potential impact, and any suggested fix.

### Response SLA

| Severity | Acknowledgment | Fix Target |
|----------|---------------|------------|
| Critical (CVSS >= 9.0) | 24 hours | 7 days |
| High (CVSS 7.0-8.9) | 48 hours | 14 days |
| Medium (CVSS 4.0-6.9) | 5 days | 30 days |
| Low (CVSS < 4.0) | 10 days | 90 days |

### Scope

**In scope:**
- Authentication and session token handling (`symbolon/`)
- Credential exposure in logs or API responses
- Path traversal in tool execution or workspace file loading
- SSRF via agent tool calls
- Prompt injection leading to tool abuse
- Memory data leakage between agent namespaces

**Out of scope:**
- Social engineering
- Physical access attacks
- Issues in dependencies (report upstream; we patch promptly)

### Disclosure

After a fix ships, we publish a GitHub Security Advisory with CVE (if warranted), description, affected versions, fixed version, and credit to the reporter.

### Supported Versions

| Version | Supported |
|---------|-----------|
| 0.x (latest minor) | Yes |
| 0.x (previous minor) | Bug fixes only |
| < current - 2 minors | No |

---

## Developer Security Practices

### Pre-commit Hooks

Install pre-commit and gitleaks to catch secrets and PII before they reach git history:

```bash
pip install pre-commit    # or: brew install pre-commit
brew install gitleaks      # or: https://github.com/gitleaks/gitleaks/releases

pre-commit install
```

`.gitleaks.toml` at the repo root defines custom rules for Anthropic API keys, Signal CLI passwords, phone numbers, and JWT tokens on top of the default gitleaks ruleset.

Manual scan:

```bash
gitleaks detect --config .gitleaks.toml --no-git --source . -v
```

### Commit Signing

All commits to `main` should be signed.

**SSH signing (recommended):**

```bash
ssh-keygen -t ed25519 -C "your-email@example.com"
git config --global gpg.format ssh
git config --global user.signingkey ~/.ssh/id_ed25519.pub
git config --global commit.gpgsign true
# Add public key to GitHub: Settings -> SSH and GPG keys -> New SSH key -> Signing Key
```

**GPG signing (alternative):**

```bash
gpg --full-generate-key
gpg --list-secret-keys --keyid-format=long  # get key ID
git config --global user.signingkey YOUR_KEY_ID
git config --global commit.gpgsign true
# Add public key to GitHub: Settings -> SSH and GPG keys -> New GPG key
```

### Secrets

- Never commit API keys, passwords, or tokens to tracked files
- Runtime secrets go in environment variables or `instance/config/credentials/` (gitignored)
- Anthropic key: `ANTHROPIC_API_KEY` environment variable
- JWT secret: generated at runtime, never persisted
- Accidental commit? Rotate immediately, scrub with `git filter-repo` or BFG Repo-Cleaner

### PII

- Phone numbers, names, and addresses never appear in tracked files
- `instance/` is gitignored - all personal data stays there
- Never log full phone numbers or message content
- Test data uses synthetic identities (alice, bob, acme.corp, 192.168.1.100)
- CI PII scanner (`.github/pii-patterns.txt`) rejects commits with personal data patterns

### Dependency Auditing

- `cargo audit` runs in CI on every PR and weekly
- `cargo deny` enforces license compatibility and bans specific crates
- Dependabot creates PRs for vulnerable dependencies automatically

### Branch Protection

Recommended `main` branch settings:

- Require pull request before merging
- Require status checks to pass (CI must be green)
- Require review from Code Owners
- Do not allow force pushes
- Do not allow deletions
