# R1602: OAuth Token Returns 400 — Root Cause Investigation

**Date:** 2026-03-19
**Author:** Research agent
**Status:** Final
**Closes:** #1602

---

## Executive Summary

The OAuth 400 errors had **two distinct root causes** at different points in the implementation, both now resolved. The first (fixed earlier, pre-#1744) was a wrong endpoint URL and wrong Content-Type on the token refresh request itself. The second (fixed by PR #1744) was a spurious `anthropic-beta: oauth-2025-04-20` header being injected into Messages API calls whenever the credential source was OAuth. That header is an internal Claude Code implementation detail not recognized by the public Anthropic Messages API; sending it causes `400 invalid_request_error: Unexpected value(s) oauth-2025-04-20 for the anthropic-beta header`.

The current implementation in `crates/symbolon/src/credential.rs` and `crates/hermeneus/src/anthropic/client.rs` is correct post-#1744. This document records the full root cause analysis to prevent regression.

---

## 1. Issue Description

Issue #1602 reports: OAuth token refresh returns 400 despite the implementation matching Claude Code's OAuth flow. PR #1744 was described as removing "the bad beta header."

The question is: what specifically caused the 400, and what does "matching CC's exact flow" actually require?

---

## 2. Root Cause 1 — Wrong Endpoint URL and Content-Type (Pre-#1744)

This was the earlier bug, fixed in commit `948dc7ed` before PR #1744.

### 2.1 Wrong URL

The refresh request was sent to `https://platform.claude.com/v1/oauth/token`.

The correct endpoint is `https://console.anthropic.com/v1/oauth/token`.

The Anthropic OAuth server lives under `console.anthropic.com`, not `platform.claude.com`. The two domains serve different purposes: `platform.claude.com` is the developer documentation and playground; `console.anthropic.com` is the account and OAuth management endpoint.

The current code has an explicit comment documenting this:

```rust
// crates/symbolon/src/credential.rs:39
/// OAuth token refresh endpoint.
// WHY: must match console.anthropic.com, not platform.claude.com
const OAUTH_TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
```

### 2.2 Wrong Content-Type

The original implementation sent the refresh request as `application/json`:

```rust
// Before fix:
client.post(OAUTH_TOKEN_URL).json(&payload).send().await
```

OAuth 2.0 token endpoints (RFC 6749 §4.1.3 and §6) require `application/x-www-form-urlencoded`. Sending a JSON body causes the server to return 400 because it cannot parse the grant parameters from the wrong content type.

After the fix:

```rust
// crates/symbolon/src/credential.rs — do_refresh() (current)
let body = format!(
    "grant_type=refresh_token&refresh_token={refresh_token}&client_id={OAUTH_CLIENT_ID}",
);
client
    .post(OAUTH_TOKEN_URL)
    .header("Content-Type", "application/x-www-form-urlencoded")
    .body(body)
    .timeout(Duration::from_secs(30))
    .send()
    .await
```

This matches RFC 6749 exactly. No other headers are required for the token refresh endpoint.

---

## 3. Root Cause 2 — Spurious `anthropic-beta: oauth-2025-04-20` on Messages API (PR #1744)

### 3.1 The bug

Commit `0e764418` ("OAuth tokens need Bearer header + beta flag") introduced conditional logic in the `build_headers()` function in `crates/hermeneus/src/anthropic/client.rs`. When `CredentialSource::OAuth`, the function was inserting:

```rust
headers.insert(
    reqwest::header::AUTHORIZATION,
    HeaderValue::from_str(&format!("Bearer {}", credential.secret))?,
);
headers.insert(
    "anthropic-beta",
    HeaderValue::from_static("oauth-2025-04-20"),
);
```

The `Authorization: Bearer` change is correct. The `anthropic-beta: oauth-2025-04-20` line is wrong.

### 3.2 Why the header is wrong

The `anthropic-beta` header is for opting into **Messages API feature flags** (e.g., `files-api-2025-04-14`, `computer-use-2024-10-22`). It has no relationship to authentication method.

The `oauth-2025-04-20` value is not documented in Anthropic's public beta header reference and is not accepted by the public Messages API. When sent:

```
400 Bad Request
{"type":"error","error":{"type":"invalid_request_error",
  "message":"Unexpected value(s) oauth-2025-04-20 for the anthropic-beta header"}}
```

### 3.3 Where `oauth-2025-04-20` comes from

This value is an **internal Claude Code signaling mechanism**, not a public API feature. Evidence:

1. **Claude Code GitHub issue #13770**: Starting with Claude Code 2.0.65, the header `anthropic-beta: oauth-2025-04-20` began appearing in all requests when OAuth was active. Users routing through LiteLLM → Vertex AI immediately received `400: Unexpected value(s) oauth-2025-04-20 for the anthropic-beta header`. Vertex AI's error message is explicit.

2. **Official Anthropic Python SDK** (`_client.py`): The `_bearer_auth` property returns only `{"Authorization": f"Bearer {auth_token}"}`. No `anthropic-beta` field.

3. **Official Anthropic TypeScript SDK** (`src/client.ts`): `bearerAuth()` returns only `{ Authorization: 'Bearer {token}' }`. No `anthropic-beta` field.

The header is meaningful when Claude Code hits Anthropic's own production backend (which silently accepts or ignores it for internal routing), but not on the public API or any third-party backend.

### 3.4 The fix (PR #1744)

PR #1744 removed the four lines inserting `anthropic-beta: oauth-2025-04-20`. The correct header set for an OAuth Bearer credential on the Messages API is:

```
Authorization: Bearer sk-ant-oat...
anthropic-version: 2023-06-01
content-type: application/json
```

No `anthropic-beta` header unless a feature flag is explicitly required by the operation.

---

## 4. Correct Header Sets (Canonical Reference)

### 4.1 Messages API with OAuth Bearer credential

```
Authorization: Bearer sk-ant-oat01...
anthropic-version: 2023-06-01
content-type: application/json
```

### 4.2 Messages API with API key

```
x-api-key: sk-ant-api03...
anthropic-version: 2023-06-01
content-type: application/json
```

### 4.3 Token refresh (`/v1/oauth/token`)

```
Content-Type: application/x-www-form-urlencoded
```

Body (form-urlencoded):
```
grant_type=refresh_token&refresh_token={token}&client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e
```

No `Authorization`, no `anthropic-version`, no `anthropic-beta`.

---

## 5. Full OAuth 400 Cause Checklist

For completeness, every reason a `POST /v1/oauth/token` or `POST /v1/messages` call can return 400 in the context of this implementation:

| Cause | Endpoint | Fixed? | Notes |
|---|---|---|---|
| Wrong Content-Type (JSON instead of form) | `/v1/oauth/token` | Yes (commit `948dc7ed`) | RFC 6749 requires form-encoded |
| Wrong endpoint URL (`platform.claude.com`) | `/v1/oauth/token` | Yes (commit `948dc7ed`) | Must use `console.anthropic.com` |
| Spurious `anthropic-beta: oauth-2025-04-20` | `/v1/messages` | Yes (PR #1744) | Internal Claude Code header, not public API |
| Expired or already-rotated refresh token | `/v1/oauth/token` | Handled | Circuit breaker prevents thrashing |
| Missing `grant_type` parameter | `/v1/oauth/token` | N/A | Present in current implementation |
| PKCE `code_verifier` required | `/v1/oauth/token` | N/A | Only required for `authorization_code` grant, not `refresh_token` |
| `redirect_uri` mismatch | `/v1/oauth/token` | N/A | Not required for `refresh_token` grant |
| Wrong `client_id` | `/v1/oauth/token` | N/A | Using Claude Code production ID `9d1c250a-e61b-44d9-88ed-5944d1962f5e` |

---

## 6. Stale Documentation

Two files contain references to the removed header that should be updated to reflect the post-#1744 state:

- `crates/hermeneus/CLAUDE.md` line 31: `"OAuth auth": "Bearer" + "anthropic-beta: oauth-2025-04-20"` — should read `Bearer` only.
- `docs/research/R1457-standalone-oauth.md` lines ~143, ~173, ~210: references to `anthropic-beta: oauth-2025-04-20` in the OAuth design notes should note that the header was found to be incorrect and was removed.

These do not affect runtime behavior (the code is correct) but will confuse future developers reading the documentation.

---

## 7. Regression Prevention

To prevent reintroduction of the bad header, consider adding an integration test or assertion in `build_headers()`:

```rust
// Defensive assertion: OAuth credentials must not set anthropic-beta
#[cfg(test)]
fn assert_no_beta_header_for_oauth(headers: &HeaderMap) {
    if let Some(val) = headers.get("anthropic-beta") {
        panic!(
            "anthropic-beta header set for OAuth credential: {:?} — \
             this header is for feature flags only and causes 400 on the public API",
            val
        );
    }
}
```

Alternatively, document the constraint in the code comment at the `build_headers()` function.

---

## 8. Current Implementation Status

Post-PR #1744, the implementation in `crates/symbolon/src/credential.rs` is correct:

- Token refresh: `console.anthropic.com`, form-encoded, no extra headers ✓
- Client ID: `9d1c250a-e61b-44d9-88ed-5944d1962f5e` ✓
- Background refresh: circuit breaker, 1-hour threshold, 60-second check interval ✓
- JWT expiry detection: parses `exp` claim without signature verification ✓
- Encrypted credential file support: AES-256-GCM with sidecar key ✓
- Three layout formats: flat, wrapped (`claudeAiOauth`), encrypted ✓

The credential provider chain (env → file → refreshing) correctly falls through on expired or missing credentials.

---

## 9. Sources

- `crates/symbolon/src/credential.rs` — current `do_refresh()` implementation
- `crates/hermeneus/src/anthropic/client.rs` — `build_headers()` post-#1744
- Claude Code GitHub issue #13770 — user reports of `oauth-2025-04-20` causing 400 on Vertex AI
- Anthropic Python SDK `_client.py` — `_bearer_auth` returns `Authorization: Bearer` only
- Anthropic TypeScript SDK `src/client.ts` — `bearerAuth()` returns `Authorization: Bearer` only
- Anthropic beta header docs — `platform.claude.com/docs/en/api/beta-headers` (no `oauth-2025-04-20` entry)
- RFC 6749 §6 — `refresh_token` grant requires `application/x-www-form-urlencoded`
