# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.x (latest minor) | ✓ |
| 0.x (previous minor) | Bug fixes only |
| < current - 2 minors | ✗ |

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Report privately via GitHub's built-in security advisory system:
→ https://github.com/forkwright/aletheia/security/advisories/new

Or use the GitHub Security Advisory link above.

Include:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Any suggested fix

## Response SLA

| Severity | Acknowledgment | Fix Target |
|----------|---------------|------------|
| Critical (CVSS ≥ 9.0) | 24 hours | 7 days |
| High (CVSS 7.0–8.9) | 48 hours | 14 days |
| Medium (CVSS 4.0–6.9) | 5 days | 30 days |
| Low (CVSS < 4.0) | 10 days | 90 days |

## Scope

**In scope:**
- Authentication and session token handling (`symbolon/`)
- Credential exposure in logs or API responses
- Path traversal in plugin loader (`prostheke/`) or export (`portability/`)
- SSRF via agent tool calls
- Prompt injection leading to tool abuse
- Memory data leakage between agent namespaces

**Out of scope:**
- Social engineering
- Physical access attacks
- Issues in dependencies (report to upstream; we'll patch promptly)

## Disclosure Policy

After a fix is released, we will publish a GitHub Security Advisory with:
- CVE (requested if warranted)
- Description
- Affected versions
- Fixed version
- Credit to reporter (if desired)

## Credential Audit (2026-02-27)

Credential scan performed before v1.4 public release using:

```
git log --all --oneline -S "sk-ant"
git log --all --oneline -S "sk-ant-api"
git log --all --oneline -S "ghp_"
git log --all --oneline -S "ANTHROPIC_API_KEY="
git log --all --oneline -S "password"
```

**Result: No actual credential values found in tracked git history.**

All matches were:
- Feature code for credential management (SecretRef spec, credential UI)
- Config template/example files with placeholder values
- Git commit metadata (author email in commit records)

The file `shared/config/aletheia.env` contains real credentials but is excluded
from git tracking via `.gitignore` (`*.env` pattern).

If you discover credentials in git history after forking: use `git filter-repo` or
BFG Repo-Cleaner to rewrite history and rotate the exposed credentials immediately.
