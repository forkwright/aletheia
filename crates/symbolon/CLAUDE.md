# symbolon

Authentication and authorization: JWT sessions, API keys, Argon2id passwords, OAuth credential refresh, RBAC. 5.3K lines.

## Read first

1. `src/credential.rs`: CredentialChain, RefreshingCredentialProvider, FileCredentialProvider (LLM API key resolution)
2. `src/jwt.rs`: JwtManager, JwtConfig (HS256 JWT issuance and validation using ring::hmac)
3. `src/types.rs`: Claims, Role, TokenKind, Action (shared auth types)
4. `src/auth.rs`: AuthService facade composing JWT, API keys, passwords, RBAC
5. `src/store.rs`: AuthStore (SQLite-backed credential and token storage)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `CredentialChain` | `credential.rs` | Priority-ordered credential resolution: env -> file -> OAuth refresh |
| `RefreshingCredentialProvider` | `credential.rs` | Background OAuth token refresh with circuit breaker |
| `FileCredentialProvider` | `credential.rs` | Reads credentials from encrypted JSON files on disk |
| `EnvCredentialProvider` | `credential.rs` | Reads API keys from environment variables |
| `CredentialFile` | `credential.rs` | On-disk credential format (token, refresh_token, expires_at) |
| `JwtManager` | `jwt.rs` | HS256 JWT issuance, validation, and refresh (Send + Sync) |
| `JwtConfig` | `jwt.rs` | Signing key, access/refresh TTL, issuer |
| `Claims` | `types.rs` | JWT payload: sub, role, nous_id, iss, iat, exp, jti, kind |
| `Role` | `types.rs` | RBAC roles: Operator, Agent, Readonly |
| `AuthService` | `auth.rs` | Unified facade: register, login, issue/validate tokens, check permissions |
| `AuthStore` | `store.rs` | SQLite-backed user, API key, and token persistence |

## Patterns

- **Credential chain**: Priority cascade (env -> file -> OAuth refresh) with first-match-wins semantics.
- **Circuit breaker**: Three-state (Closed/Open/HalfOpen) breaker on OAuth refresh prevents thundering herd on provider outages.
- **Background refresh**: Tokio task polls token expiry at 60s intervals, refreshes when under 1h remaining.
- **File mtime watch**: FileCredentialProvider detects external credential file updates every 30s.
- **Encryption at rest**: AES-256-GCM via `encrypt` module for credential files. `enc:` prefix in config triggers transparent decryption.
- **Clock skew tolerance**: 30s leeway on token expiry checks to handle NTP drift.

## Common tasks

| Task | Where |
|------|-------|
| Add credential source | `src/credential.rs` (implement `CredentialProvider` trait from koina) |
| Add RBAC role | `src/types.rs` (Role enum) + `src/auth.rs` (permission checks) |
| Modify JWT claims | `src/types.rs` (Claims struct) + `src/jwt.rs` (encode/decode) |
| Add auth store table | `src/store.rs` (AuthStore, schema migrations) |
| Modify circuit breaker | `src/circuit_breaker.rs` (CircuitBreakerConfig thresholds) |

## Dependencies

Uses: koina, argon2, ring, reqwest, rusqlite
Used by: pylon, diaporeia, aletheia (binary)
