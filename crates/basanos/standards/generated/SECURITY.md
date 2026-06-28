# Security Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [RUST/plain-string-secret](#rustplain-string-secret)
- [SECURITY/config-write-no-perms](#securityconfig-write-no-perms)
- [SECURITY/credential-perms](#securitycredential-perms)
- [SECURITY/destructive-git-force-push-main](#securitydestructive-git-force-push-main)
- [SECURITY/hardcoded-api-key](#securityhardcoded-api-key)
- [SECURITY/hardcoded-aws-access-key](#securityhardcoded-aws-access-key)
- [SECURITY/hardcoded-loopback-url](#securityhardcoded-loopback-url)
- [SECURITY/hardcoded-oauth-token](#securityhardcoded-oauth-token)
- [SECURITY/hardcoded-tmp](#securityhardcoded-tmp)

## `RUST/plain-string-secret` {#rustplain-string-secret}

- Severity: `warning`
- Scope: `language:rust`
- Enforcer: `RUST/plain-string-secret`

Secret-bearing fields and parameters must not use plain `String`. Use a redaction-aware secret type so credentials, tokens, keys, and passwords do not leak through debug output, logs, or serialization.

### Examples

**Good:** Store secret-bearing fields in a redaction-aware type.

```text
pub struct Credentials {
    api_token: SecretString,
}
```

**Bad:** Store a secret-sounding field as a plain string.

```text
pub struct Credentials {
    api_token: String,
}
```

## `SECURITY/config-write-no-perms` {#securityconfig-write-no-perms}

- Severity: `error`
- Scope: `language:rust`
- Enforcer: `SECURITY/config-write-no-perms`
- See also: `SECURITY/credential-perms`

Rust code must not write config, credential, secret, token, or key paths with `std::fs::write` unless the write path also applies restricted file permissions. Sensitive files should be owner-readable only so default umasks cannot expose credentials.

### Examples

**Good:** Set restricted permissions when writing credential-bearing files.

```text
std::fs::write(&path, config)?;
std::fs::set_permissions(&path, Permissions::from_mode(0o600))?;
```

**Bad:** Write a sensitive config file without tightening permissions.

```text
std::fs::write("config/credentials.toml", token)?;
```

## `SECURITY/credential-perms` {#securitycredential-perms}

- Severity: `error`
- Scope: `universal`
- Enforcer: `SECURITY/credential-perms`
- See also: `SECURITY/config-write-no-perms`

Credential files must be owner-readable and owner-writable only. Files under credential paths with broader permissions can expose API keys, tokens, or service-account material to other users and processes.

### Examples

**Good:** Keep credential files readable and writable only by the owner.

```text
chmod 600 credentials/service-account.json
```

**Bad:** Leave a credential file group-readable or world-readable.

```text
chmod 644 credentials/service-account.json
```

## `SECURITY/destructive-git-force-push-main` {#securitydestructive-git-force-push-main}

- Severity: `error`
- Scope: `language:shell`
- Enforcer: `SECURITY/destructive-git-force-push-main`

`git push --force` must not target `origin main` or `origin master`. Shared default branches need reviewable revert commits or lease-checked feature-branch rewrites so contributors do not silently lose commits they have already based work on.

### Examples

**Good:** Use the lease-checked force variant away from shared default branches.

```text
git push --force-with-lease origin feature/destructive-op-guard
```

**Bad:** Force-push directly to the shared main branch.

```text
git push --force origin main
```

## `SECURITY/hardcoded-api-key` {#securityhardcoded-api-key}

- Severity: `error`
- Scope: `universal`
- Enforcer: `SECURITY/hardcoded-api-key`

API keys must not be embedded directly in source code. Load credentials from environment variables or a secrets manager so they stay out of version control, logs, generated artifacts, and reusable fixtures.

### Examples

**Good:** Read API credentials from process configuration.

```text
let api_key = std::env::var("ANTHROPIC_API_KEY")?;
```

**Bad:** Embed an API key literal in source code.

```text
let api_key = "hardcoded credential literal";
```

## `SECURITY/hardcoded-aws-access-key` {#securityhardcoded-aws-access-key}

- Severity: `error`
- Scope: `universal`
- Enforcer: `SECURITY/hardcoded-aws-access-key`
- See also: `SECURITY/credential-perms`

AWS access key identifiers must not be embedded as source literals. Load AWS credentials from environment variables, IAM roles, or a secrets manager so credentials do not leak through source control, logs, or generated artifacts.

### Examples

**Good:** Load AWS credentials from the execution environment.

```text
let access_key = std::env::var("AWS_ACCESS_KEY_ID")?;
```

**Bad:** Commit an AWS access key identifier as a source literal.

```text
let access_key = "AKIAABCDEFGHIJKLMNOP";
```

## `SECURITY/hardcoded-loopback-url` {#securityhardcoded-loopback-url}

- Severity: `error`
- Scope: `language:rust`
- Enforcer: `SECURITY/hardcoded-loopback-url`

Rust source must not embed `http://localhost` or `http://127.0.0.1` service URLs directly. Resolve loopback endpoints from `dispatch-config.toml`, environment, or a shared config loader so service wiring remains rotatable without recompilation.

### Examples

**Good:** Resolve a loopback service URL from configuration.

```text
let url = config.embed_base_url.clone();
```

**Bad:** Embed a localhost service URL directly in production Rust code.

```text
let url = "http://localhost:3000/v1";
```

## `SECURITY/hardcoded-oauth-token` {#securityhardcoded-oauth-token}

- Severity: `error`
- Scope: `universal`
- Enforcer: `SECURITY/hardcoded-oauth-token`
- See also: `SECURITY/credential-perms`

OAuth tokens must not be embedded as source literals. Load tokens from environment variables or a secrets manager so credentials do not leak through source control, logs, or generated artifacts.

### Examples

**Good:** Load OAuth tokens from the runtime environment.

```text
let token = std::env::var("OAUTH_TOKEN")?;
```

**Bad:** Embed an OAuth token literal in source code.

```text
let token = "sk-ant-oat01-<redacted-token>";
```

## `SECURITY/hardcoded-tmp` {#securityhardcoded-tmp}

- Severity: `error`
- Scope: `universal`
- Enforcer: `SECURITY/hardcoded-tmp`

Shell scripts must not hardcode predictable paths under `/tmp`. Use `mktemp` or another temporary-file API so file creation is race-safe and does not collide with attacker-controlled symlinks or stale state.

### Examples

**Good:** Create temporary paths with a race-safe temporary-file helper.

```text
tmp=$(mktemp -d)
```

**Bad:** Hardcode a predictable path under /tmp.

```text
cache_dir=/tmp/kanon-cache
```

