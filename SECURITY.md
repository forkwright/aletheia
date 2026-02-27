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

Or email: cody.kickertz@pm.me

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
