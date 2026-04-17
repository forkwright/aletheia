# Phase 07: Auth and sessions

## Goal
JWT-based authentication, session management, and RBAC for multi-user instances.

## Success criteria
- Login flow issues signed JWT with 24h expiry
- Session store supports revocation without restart
- RBAC middleware rejects unauthorized requests with 403
- Password hashing uses Argon2id with OWASP-recommended parameters

## Falsification

| Criterion | Falsifier |
|-----------|-----------|
| Login flow issues signed JWT with 24h expiry | Token decoded shows expiry > 24h or invalid signature |
| Session store supports revocation without restart | Revoked token is still accepted by API after 5s propagation |
| RBAC middleware rejects unauthorized requests with 403 | Request with insufficient role returns 200 OK |
| Password hashing uses Argon2id with OWASP-recommended parameters | Hash parameters deviate from OWASP 2023 recommendations |

## Scope

### In scope
- symbolon crate: JWT auth, sessions, RBAC
- Session token rotation on privilege escalation
- Keyring integration (optional feature)

### Out of scope
- OAuth2 or SAML SSO
- Multi-factor authentication

## Requirements
- REQ-01: JWT secret is rotatable without invalidating all sessions
- REQ-02: Session metadata includes IP hash and user agent for audit
- REQ-03: RBAC roles are hierarchical (admin > operator > viewer)
- REQ-04: Password minimum length is 16 characters

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Token format | JWT over opaque tokens | Stateless validation, no DB round-trip |
| Hash algorithm | Argon2id over bcrypt | Memory-hard, resistant to GPU attacks |

## Open questions
- Should we add API key support for service accounts? (Resolved: yes, in Phase 09)

## Dependencies
- Phase 06 complete
- Cryptographic random number source
