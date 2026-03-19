# R1457: Standalone OAuth Login (No Claude Code Dependency)

**Date:** 2026-03-19
**Author:** Research agent
**Status:** Final
**Closes:** #1457

---

## Executive Summary

The current `symbolon` credential system is wired to the Claude Code OAuth client (`9d1c250a-e61b-44d9-88ed-5944d1962f5e`). This couples every aletheia deployment to Claude Code as the OAuth client. The goal is a first-party OAuth 2.0 PKCE flow so operators can authenticate directly — via browser or CLI — without requiring Claude Code to be installed.

**Recommendation: Implement.** The primitives already exist in `symbolon` (JWT issuance, token storage, credential file format). The missing piece is a PKCE authorization code flow with a loopback redirect server. Estimated scope: new `aletheia login` subcommand + PKCE helper in `symbolon`. No changes to `hermeneus` or `pylon`.

---

## 1. Problem Statement

Aletheia's Anthropic credential today lives at `instance/config/credentials/claude_ai.json` and is only useful if:
1. The user is already logged in via Claude Code (which writes the wrapped `claudeAiOauth` format), or
2. A static API key is provisioned manually.

This creates two problems:

- **Deployment friction:** Headless servers and CI environments cannot run Claude Code. They must use static API keys, which do not rotate automatically.
- **Coupling:** The OAuth client ID is hardcoded to Claude Code's production client. Aletheia cannot request its own OAuth scopes or appear as its own application in the Anthropic console.

Standalone OAuth solves both: operators get a browser-based or device-code login that issues a refresh token scoped to aletheia, stored in the existing encrypted credential file.

---

## 2. Proposed Approach

### 2.1 OAuth 2.0 PKCE Flow

```
aletheia login
  │
  ├─ generate code_verifier (32 random bytes, base64url)
  ├─ compute code_challenge = BASE64URL(SHA256(code_verifier))
  ├─ bind loopback HTTP server on 127.0.0.1:{random port}
  ├─ open browser to:
  │    https://console.anthropic.com/oauth/authorize
  │      ?response_type=code
  │      &client_id={ALETHEIA_CLIENT_ID}
  │      &redirect_uri=http://127.0.0.1:{port}/callback
  │      &scope=read:credentials write:credentials offline_access
  │      &code_challenge={code_challenge}
  │      &code_challenge_method=S256
  │      &state={csrf_token}
  │
  ├─ wait for GET /callback?code=...&state=...
  ├─ validate state == csrf_token
  └─ POST https://console.anthropic.com/v1/oauth/token
       client_id={ALETHEIA_CLIENT_ID}
       grant_type=authorization_code
       code={code}
       code_verifier={code_verifier}
       redirect_uri=http://127.0.0.1:{port}/callback
```

Token response writes to `instance/config/credentials/claude_ai.json` using the existing `CredentialFile` encrypted format.

### 2.2 Client Registration

Aletheia needs its own OAuth client registered with Anthropic:

| Field | Value |
|---|---|
| Client type | Public (no client secret — PKCE only) |
| Redirect URIs | `http://127.0.0.1` (loopback, any port per RFC 8252 §7.3) |
| Grant types | `authorization_code`, `refresh_token` |
| Scopes | `read:credentials`, `offline_access` (TBD — depends on Anthropic's published scope list) |
| Token endpoint auth | `none` (public client) |

The client ID is embedded in `symbolon` as a compile-time constant (parallel to the existing `CLAUDE_CODE_CLIENT_ID`).

**Prerequisite:** Anthropic must offer a published OAuth registration path for third-party clients. As of the research date, only the `claudeAiOauth` client ID is documented. This is the primary blocker and must be confirmed before implementation begins.

### 2.3 Headless / Device Code Fallback

For servers without a browser, implement RFC 8628 Device Authorization Grant:

```
POST https://console.anthropic.com/oauth/device/code
  client_id={ALETHEIA_CLIENT_ID}
  scope=...

→ { device_code, user_code, verification_uri, expires_in, interval }

aletheia login --headless
  Prints: "Visit https://... and enter code ABCD-1234"
  Polls POST /v1/oauth/token (grant_type=device_code) every {interval}s
  On success: writes credential file
```

This replaces the manual "paste your API key" flow for CI and containers.

### 2.4 Token Storage

Reuse the existing `CredentialFile` structure in `symbolon/src/credential.rs`:

```rust
// New variant alongside existing claude_ai_oauth
pub enum CredentialKind {
    AletheiaOauth {
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<Timestamp>,
        scope: String,
        client_id: String,
    },
    ClaudeCodeOauth { ... },  // keep existing
    ApiKey { ... },           // keep existing
}
```

The encrypted file format (`ALETHEIA_ENC_V1:` prefix, XChaCha20Poly1305) is reused without change. The `CredentialManager` refresh logic already handles expiry detection and token refresh — it needs only a new branch for the `AletheiaOauth` variant pointing to the new token endpoint.

### 2.5 CLI Subcommand

New `aletheia login` subcommand in `crates/aletheia/src/commands/login.rs`:

```
aletheia login [--headless] [--client-id ID] [--scope SCOPE...]
aletheia logout
aletheia credential status   # already exists, extend to show auth method
```

The `--client-id` override allows operators to register their own Anthropic application (if Anthropic supports it) without recompiling.

### 2.6 Scope Requirements

Minimum required scopes (pending Anthropic scope documentation):

| Scope | Purpose |
|---|---|
| `offline_access` | Receive a refresh token (required for long-lived sessions) |
| `read:credentials` | Read Anthropic API access |
| `model:inference` | Send requests to `/v1/messages` (if scope-gated) |

The existing `anthropic-beta: oauth-2025-04-20` request header in `hermeneus` must match whatever beta header the new client ID requires.

---

## 3. Alternatives Considered

### 3.1 Use the Claude Code Client ID Forever

Keep using `9d1c250a-e61b-44d9-88ed-5944d1962f5e` and instruct users to run `claude /login` first.

**Rejected.** This makes Claude Code a hard dependency for all deployments, prevents headless use, and means aletheia appears as "Claude Code" in Anthropic's usage dashboard.

### 3.2 Static API Key Only

Remove OAuth entirely; require `ANTHROPIC_API_KEY` env var.

**Rejected.** Static keys don't rotate, and aletheia's operator UI already supports the OAuth refresh flow. Removing it would regress existing users.

### 3.3 Piggyback on the Existing Token Refresh

Intercept the refresh token from Claude Code's file and use it as if it were our own.

**Rejected.** This is fragile (depends on file location and format), violates the spirit of OAuth (the refresh token belongs to Claude Code), and will break if Anthropic binds refresh tokens to client IDs.

---

## 4. Open Questions

1. **Does Anthropic support public OAuth client registration for third parties?** The entire proposal depends on this. If not, the headless device-code flow may be possible with the existing client ID but the browser PKCE flow cannot use a custom client ID.

2. **What scopes does Anthropic publish?** The beta header (`oauth-2025-04-20`) is undocumented beyond the Claude Code integration. Scope names are guesses based on common OAuth conventions.

3. **Token binding:** Does Anthropic bind refresh tokens to client IDs? If yes, the existing Claude Code refresh tokens cannot be used with a new client ID (correct behavior). If no, there is a security implication worth raising with Anthropic.

4. **Redirect URI validation:** RFC 8252 §7.3 requires loopback redirect URIs to be accepted without port matching. Confirm Anthropic's authorization server implements this.

5. **PKCE requirement for device flow:** Some authorization servers require PKCE even for device code grants. Confirm whether to include `code_challenge` in the device authorization request.

6. **Multi-instance:** If an operator runs two aletheia instances under the same Anthropic account, are refresh tokens independent per instance? (They should be, since each PKCE flow generates an independent authorization code.)

---

## 5. Implementation Sketch

```
crates/symbolon/src/
  oauth/
    pkce.rs          # verifier, challenge, state generation
    loopback.rs      # single-request HTTP server for redirect
    device.rs        # RFC 8628 device authorization + polling
    client.rs        # shared token exchange logic
  credential.rs      # add AletheiaOauth variant

crates/aletheia/src/commands/
  login.rs           # new: aletheia login / aletheia logout
  credential.rs      # extend status to show method
```

The loopback server is a minimal one-shot `tokio::net::TcpListener` — no Axum — that accepts one request, extracts the `code` and `state` query parameters, renders a success page, and shuts down.

---

## 6. References

- RFC 7636 — Proof Key for Code Exchange (PKCE)
- RFC 8252 — OAuth 2.0 for Native Apps (loopback redirect)
- RFC 8628 — OAuth 2.0 Device Authorization Grant
- Anthropic OAuth beta header: `anthropic-beta: oauth-2025-04-20`
- Existing credential implementation: `crates/symbolon/src/credential.rs`
