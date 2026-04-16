# API Versioning Policy

This document describes how the Aletheia HTTP API is versioned, what stability guarantees apply, and how breaking changes are managed.

## Current state

- **Stable API** lives under `/api/v1/`. This includes session management, nous, configuration, knowledge, and planning endpoints.
- **Infrastructure endpoints** are unversioned: `/api/health`, `/metrics`, and `/api/docs/openapi.json`. These may change without a version bump.

The generated OpenAPI spec served at `/api/docs/openapi.json` is the source of truth for the current contract.

## Stability tiers

| Tier | Path prefix | Guarantee |
|------|-------------|-----------|
| **Stable** | `/api/v1/` | Backwards-compatible evolution only. Breaking changes require a new major version (v2). |
| **Infrastructure** | unversioned | No stability guarantee. May change without notice. |

## Breaking change policy

A change to a **Stable** endpoint is considered breaking when it could cause an existing correctly-written client to fail or behave incorrectly.

| Change type | Classification | Action |
|-------------|----------------|--------|
| Adding new optional request or response fields | Non-breaking | Evolve v1 |
| Adding new endpoints | Non-breaking | Evolve v1 |
| Removing or renaming fields | Breaking | Introduce v2 |
| Changing field semantics (e.g. integer seconds → milliseconds) | Breaking | Introduce v2 |
| Changing status codes for existing success/failure cases | Breaking | Introduce v2 |
| Changing authentication or authorization requirements | Breaking | Introduce v2 |

## Deprecation process

Before an endpoint or field is removed, it will go through a deprecation window:

- Deprecated resources will be marked with `Deprecation` and `Sunset` response headers (see #3280).
- Documentation and the OpenAPI spec will annotate the resource as deprecated.
- Deprecated resources will remain functional for at least one full release cycle after the deprecation notice is published.

## Supporting old versions

When a new major API version is released (e.g. v2), the previous version (v1) will continue to be supported for **6 months**. This gives clients a defined migration window. After the support period ends, v1 routes may return `410 Gone` with a migration hint, similar to how old unversioned `/api/nous` paths are handled today.

## Version negotiation

Aletheia uses **URL path-based versioning** only. Clients declare the desired version by calling the appropriate prefixed path (e.g. `/api/v1/sessions`).

The server does **not** negotiate versions via `Accept` headers or custom media types. This keeps routing explicit and caching straightforward.
