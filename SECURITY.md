# Security Policy

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Report privately via GitHub security advisories:
→ https://github.com/CKickertz/ergon/security/advisories/new

Or email: cody.kickertz@pm.me

Include: description, steps to reproduce, potential impact, and any suggested fix.

## Response SLA

| Severity | Acknowledgment | Fix Target |
|----------|---------------|------------|
| Critical (CVSS ≥ 9.0) | 24 hours | 7 days |
| High (CVSS 7.0–8.9) | 48 hours | 14 days |
| Medium (CVSS 4.0–6.9) | 5 days | 30 days |
| Low (CVSS < 4.0) | 10 days | 90 days |

## Scope

**In scope:**
- Authentication and session token handling
- Credential exposure in logs or API responses
- Path traversal in plugin loader or export
- SSRF via agent tool calls
- Prompt injection leading to tool abuse
- Memory data leakage between agent namespaces

**Out of scope:**
- Social engineering
- Physical access attacks
- Issues in upstream dependencies (report to upstream; we patch promptly)

## Disclosure

After a fix ships, we publish a GitHub Security Advisory with affected versions, fix version, and reporter credit (if desired).
