# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| main    | Yes       |

## Reporting a Vulnerability

If you discover a security vulnerability, please report it via [GitHub Security Advisories](https://github.com/forkwright/aletheia/security/advisories/new).

Do not open a public issue for security vulnerabilities.

## Scanning

This repository uses:
- **Dependabot** — automated dependency updates (Cargo, GitHub Actions)
- **CodeQL** — static analysis for GitHub Actions workflows
- **TruffleHog + Gitleaks** — secret detection in git history
- **cargo audit + cargo deny** — Rust dependency vulnerability and license scanning
