# Testing Standards

> Additive to README.md. Read that first. Everything here covers test philosophy, strategy, and test data policy.

---

## Behavior Over Implementation

Test what the code **does**, not how it does it. If a refactor breaks your tests but doesn't break behavior, your tests are wrong.

- One logical assertion per test
- Descriptive names: `returns_empty_when_session_has_no_turns`, not `test_add` or `it_works`
- Same-directory test files (colocated with source, not in a separate tree)

## Property-Based Testing

Use property tests for:
- Serialization round-trips (`deserialize(serialize(x)) == x`)
- Algebraic properties (commutativity, associativity, idempotency)
- State machine invariants
- Edge case discovery at scale

Example tests document expected behavior. Property tests catch the unexpected.

## What to Test

Focus testing effort on behavior with consequences:
- Boundary conditions (empty, one, many, max, overflow)
- Error paths (invalid input, unavailable service, timeout, permission denied)
- State transitions (especially concurrent access to shared state)
- Serialization round-trips (`deserialize(serialize(x)) == x`)
- Idempotency (replaying the same operation produces the same result)
- Security boundaries (authentication, authorization, input validation)

## No Coverage Targets

Coverage is a vanity metric. Test the behavior that matters. Untested code should be deliberately untested (trivial, generated, or unreachable), not accidentally missed.

## Test Data Policy

All test data must be synthetic. No real personal information in test fixtures, assertions, or examples.

**Standard test identities:**
- Users: `alice`, `bob`, `charlie`
- Emails: `alice@example.com`, `bob@example.org` (RFC 2606 reserved domains only)
- Phones: `+1-555-0100` through `+1-555-0199` (ITU reserved for fiction)
- IPs: `192.0.2.x`, `198.51.100.x`, `203.0.113.x` (RFC 5737 documentation ranges)
- IPv6: `2001:db8::/32` (RFC 3849 documentation range)
- Domains: `example.com`, `example.org`, `example.net`, `*.test` (RFC 2606/6761 reserved)

**Never use:** real names, emails, usernames, internal IPs/hostnames, personal facts, credentials, or API keys. Never copy production data into test environments.

**Test data builders:** Use builder/factory patterns with sensible defaults. Each test overrides only the fields it cares about. When a field is added to the struct, only the builder default needs updating — not every test.

**Determinism:** Any randomized test data must be seeded. The seed must be logged or persisted. Proptest regression files, hypothesis databases, and equivalent fixtures are checked into version control.
