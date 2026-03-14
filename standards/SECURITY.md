# Security Standards

> Additive to README.md. Read that first. Everything here covers credentials, input validation, output encoding, and access control.

---

## Credentials and Secrets

- No secrets in code. Not in constants, not in comments, not in test fixtures, not in config files checked into version control.
- Environment variables or secret managers for all credentials.
- `.gitignore` sensitive paths. Pre-commit hooks (gitleaks) catch accidental commits.
- Rotate credentials immediately if they ever touch version control, even on a branch that was never pushed.

## Input Boundaries

- All external input is hostile until parsed. Validate on the trusted side of the boundary. Allowlists over denylists. Validate type, range, and length.
- Parameterized queries for all SQL. No string interpolation. No exceptions.
- Size limits on all user-provided input (file uploads, text fields, API payloads). Fail before allocating.
- Canonicalize paths and encodings before validating.

## Output Encoding

Encode data for its output context. Context-appropriate escaping for HTML, shell commands, LDAP, log messages. The encoding belongs at the point of interpolation, not at the point of data entry.

## Deny by Default

Access control fails closed. If authorization state is unknown or ambiguous, deny.

## Dependencies

- `cargo-deny`, `npm audit`, `dotnet list package --vulnerable`, `pip-audit` on every CI run.
- Evaluate transitive dependencies, not just direct ones.
- No dependencies with known CVEs in production builds.

## Principle of Least Privilege

- Services run with minimum required permissions.
- API tokens scoped to the narrowest access needed.
- File permissions explicit, not inherited defaults.
