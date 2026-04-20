# symbolon

**Purpose:** Authentication and authorization: JWT sessions, API key management, Argon2id password hashing, OAuth credential refresh, and RBAC.

## Key types

| Type | Purpose |
|------|---------|
| `JwtManager` | HS256 JWT issuance, validation, and refresh |
| `AuthService` | Facade composing JWT, API keys, passwords, and RBAC |
| `CredentialChain` | Priority-ordered credential resolution: env → file → OAuth refresh |
| `Claims` | JWT payload: sub, role, nous_id, iss, iat, exp, jti, kind |
| `RefreshingCredentialProvider` | Background OAuth token refresh with circuit breaker |

## Public API surface

- `symbolon::auth` - `AuthService`, `JwtManager`, `JwtConfig`
- `symbolon::credential` - `CredentialChain`, `FileCredentialProvider`, `EnvCredentialProvider`
- `symbolon::types` - `Claims`, `Role`, `TokenKind`, `Action`

## When to look here

- When adding auth checks, new RBAC roles, or modifying token validation logic
- When resolving LLM API credentials or implementing a new credential provider
