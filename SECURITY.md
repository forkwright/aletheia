# Security

## Reporting vulnerabilities

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

### Supported versions

| Version | Supported |
|---------|-----------|
| 0.x (latest minor) | Yes |
| 0.x (previous minor) | Bug fixes only |
| < current - 2 minors | No |

## Software Bill of Materials (SBOM)

Per basanos security requirements, every dependency must be tracked in an SBOM with component name, version, PURL, license (SPDX), and hash.

### Generating SBOM locally

Run the provided script to generate a CycloneDX SBOM:

```bash
./scripts/generate-sbom.sh
```

This produces `bom.cdx.json` at the workspace root.

### SBOM in releases

On every release, CI generates two SBOMs:
- **SPDX format** via Anchore SBOM Action (`aletheia-sbom.spdx.json`)
- **CycloneDX format** via cargo-cyclonedx (`bom.cdx.json`)

Both are attached as release artifacts automatically.
