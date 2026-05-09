# symbolon

**Purpose:** Authentication and authorization for Aletheia.

## Key types

| Type | Purpose |
|------|---------|
| `AuthFacade` | Current public type or boundary; see L3/source for exact fields |
| `AuthService` | Current public type or boundary; see L3/source for exact fields |
| `JwtManager` | Current public type or boundary; see L3/source for exact fields |
| `Claims` | Current public type or boundary; see L3/source for exact fields |
| `Role` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `symbolon::auth` - public items from `src/auth.rs`
- `symbolon::circuit_breaker` - public items from `src/circuit_breaker.rs`
- `symbolon::credential/device_code` - public items from `src/credential/device_code.rs`
- `symbolon::credential/file_ops` - public items from `src/credential/file_ops.rs`
- `symbolon::credential/keyring_provider` - public items from `src/credential/keyring_provider.rs`

## When to look here

- When work touches `crates/symbolon` or downstream imports from `symbolon`.
- For exact signatures, load `_llm/L3-api-index/symbolon.md` if present, then source.

## Recent changes

AuthFacade is wired as the admin-token verification and revocation boundary; terminal UX no longer lives in the library.
