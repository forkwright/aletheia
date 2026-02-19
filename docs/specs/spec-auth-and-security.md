# Spec: Authentication & Security Hardening

**Status:** Draft  
**Author:** Syn  
**Date:** 2026-02-19  

---

## Problem

The current auth model is a static bearer token stored in `localStorage`. There is no TLS, no session management, no CSRF protection, no audit logging, and no role-based access control. The token never expires, cannot be revoked, and is transmitted in the clear over HTTP on the LAN.

This was fine for local-only development. It is not fine for a system that:
- Runs on a LAN with multiple devices connecting
- Exposes an MCP endpoint for external tool use
- Has admin API endpoints that can archive sessions, trigger distillation, trigger cron jobs, and proxy to internal services (memory sidecar)
- Serves a webchat UI over Tailscale to remote devices
- Stores conversation history, personal data, and API keys for LLM providers

### Current State — What Exists

**Gateway auth (pylon/server.ts):**
- Two modes: `token` (Bearer header) and `password` (HTTP Basic Auth)
- Single shared secret in `config.yaml` (`gateway.auth.token`)
- `timingSafeEqual` used for comparison (good)
- `/health` and `/api/branding` exempt from auth (correct)
- `/ui/*` routes exempt from auth (the UI handles its own token check — but this means static assets are public)

**Webchat auth (ui/lib/api.ts):**
- Token stored in `localStorage` under `aletheia_token`
- Sent as `Bearer` header on every API call
- Sent as `?token=` query param on SSE EventSource connections (because EventSource doesn't support custom headers)
- No expiry, no refresh, no revocation
- Token visible in browser DevTools, URL bar (SSE), and network logs

**MCP auth (pylon/mcp.ts):**
- Separate token system: `mcp-tokens.json` file with scoped tokens
- Per-token scopes (`agent:*`, `system:status`, etc.)
- `hasScope()` check per tool call
- Decent design — but tokens are static files, no rotation, no revocation

**Signal channel auth (semeion/listener.ts):**
- Allowlist-based (static phone numbers in config)
- Dynamic pairing system (challenge codes, admin approval)
- Per-account DM and group policies (`open`, `disabled`, `pairing`, `allowlist`)
- This is actually well-designed for its use case

**Rate limiting:**
- Per-IP sliding window on `/mcp/*` only
- No rate limiting on main API endpoints (`/api/sessions/send`, `/api/sessions/stream`)
- No rate limiting on admin endpoints

**Security headers:**
- `X-Content-Type-Options: nosniff` ✓
- `X-Frame-Options: DENY` ✓
- `Referrer-Policy: no-referrer` ✓
- `X-XSS-Protection: 0` ✓
- No `Content-Security-Policy`
- No `Strict-Transport-Security` (can't — no TLS)
- No `Permissions-Policy`

**TLS:** None. Everything is HTTP. The Tailscale connection provides encryption for remote access, but LAN access is unencrypted.

**CORS:** Configured but restrictive by default (empty `allowOrigins` = no cross-origin allowed).

**What's missing entirely:**
- Session-based auth with expiry
- Refresh tokens
- Token revocation
- Audit logging (who accessed what, when)
- Role separation (admin vs user vs read-only)
- CSRF protection
- TLS termination
- Input sanitization beyond basic type checks
- Memory sidecar proxy auth (gateway proxies to `http://127.0.0.1:8230` with no auth — fine if loopback-only, but worth documenting)

---

## Goals

1. **Replace static token with session-based auth.** Login → short-lived session → refresh cycle. Tokens expire and can be revoked.
2. **Add TLS.** The gateway should serve HTTPS natively with self-signed certs (auto-generated) or user-provided certs.
3. **Role-based access.** At minimum: `admin` (full access) and `user` (chat + read-only metrics). Future: per-agent scoping.
4. **Audit logging.** Every authenticated action gets logged with actor, action, target, timestamp.
5. **Security headers hardening.** CSP, HSTS (once TLS exists), Permissions-Policy.
6. **Rate limiting on all endpoints.** Not just MCP.
7. **Backward compatible.** Existing `token` mode still works for quick setup. New auth is opt-in via config.

## Non-Goals

- OAuth2 / OIDC provider integration (defer — overkill for self-hosted)
- Multi-user with separate databases (all users share the same agent ecosystem)
- E2E encryption of stored messages (SQLite encryption at rest is a separate concern)
- Changing the Signal channel auth model (it's already good)

---

## Design

### 1. Auth Modes

The config schema supports multiple auth modes. The current `token` and `password` modes remain for backward compatibility. A new `session` mode adds proper session management:

```yaml
gateway:
  auth:
    mode: "session"            # "token" | "password" | "session"

    # Token mode (existing — unchanged)
    token: "my-secret-token"

    # Session mode (new)
    session:
      secret: "..."            # HMAC signing key for session tokens (auto-generated if absent)
      accessTokenTtl: 900      # 15 minutes (seconds)
      refreshTokenTtl: 604800  # 7 days (seconds)
      maxSessions: 10          # per user — oldest evicted on overflow
      secureCookies: true      # Requires TLS. Set false for HTTP-only dev.

    # Users (new — replaces single shared token)
    users:
      - username: "cody"
        passwordHash: "$argon2id$..."   # argon2id hash
        role: "admin"
      - username: "guest"
        passwordHash: "$argon2id$..."
        role: "user"

    # Roles (new)
    roles:
      admin:
        - "*"                   # full access
      user:
        - "api:chat"            # send/stream messages
        - "api:sessions:read"   # list/read sessions
        - "api:agents:read"     # list agents, read identity
        - "api:metrics:read"    # read-only metrics
        - "api:events"          # SSE stream
        - "api:branding"        # branding endpoint
      readonly:
        - "api:sessions:read"
        - "api:agents:read"
        - "api:metrics:read"
        - "api:branding"
```

**Resolution:** If `mode: "token"`, behavior is identical to today. If `mode: "session"`, the login/refresh flow is active and the static `token` field is ignored.

### 2. Session Lifecycle

```
POST /api/auth/login       { username, password }  → { accessToken, refreshToken, expiresIn }
POST /api/auth/refresh     { refreshToken }        → { accessToken, refreshToken, expiresIn }
POST /api/auth/logout      (access token in header) → { ok: true }
GET  /api/auth/me          (access token in header) → { username, role, sessionId }
```

**Access tokens:** Short-lived (15min default), signed with HMAC-SHA256, stateless verification. Payload: `{ sub: username, role: string, sid: sessionId, iat, exp }`. Sent as `Bearer` header.

**Refresh tokens:** Longer-lived (7 days default), stored in the SQLite database (hashed), bound to a session ID. Can be revoked individually or all-at-once per user. Sent as `httpOnly` cookie (preferred) or in request body (for non-browser clients).

**Token format:** HMAC-signed JWTs. No external dependency — `node:crypto` HMAC is sufficient for self-hosted single-server deployment. Not using RSA/ECDSA because there's no multi-service verification scenario.

```typescript
// auth/tokens.ts

import { createHmac } from "node:crypto";

interface AccessTokenPayload {
  sub: string;      // username
  role: string;     // admin | user | readonly
  sid: string;      // session ID
  iat: number;      // issued at (unix seconds)
  exp: number;      // expires at (unix seconds)
}

function signToken(payload: AccessTokenPayload, secret: string): string {
  const header = base64url(JSON.stringify({ alg: "HS256", typ: "JWT" }));
  const body = base64url(JSON.stringify(payload));
  const sig = createHmac("sha256", secret).update(`${header}.${body}`).digest("base64url");
  return `${header}.${body}.${sig}`;
}

function verifyToken(token: string, secret: string): AccessTokenPayload | null {
  const [header, body, sig] = token.split(".");
  if (!header || !body || !sig) return null;

  const expected = createHmac("sha256", secret).update(`${header}.${body}`).digest("base64url");
  if (!timingSafeEqual(Buffer.from(sig), Buffer.from(expected))) return null;

  const payload = JSON.parse(Buffer.from(body, "base64url").toString()) as AccessTokenPayload;
  if (payload.exp < Math.floor(Date.now() / 1000)) return null;

  return payload;
}
```

**Why not a JWT library?** Zero dependencies for auth. The payload is simple, the signing is one HMAC call, and we control both signing and verification. A library adds surface area for no benefit.

### 3. Password Hashing

Argon2id via the `argon2` npm package (native binding, widely used). Passwords never stored in plaintext.

**CLI helper:**

```bash
aletheia auth hash-password
# Interactive prompt, outputs argon2id hash for config.yaml
```

```bash
aletheia auth add-user --username cody --role admin
# Interactive password prompt → writes to config.yaml
```

**Migration from token mode:** A CLI command generates a user entry from the existing token:

```bash
aletheia auth migrate
# Reads gateway.auth.token, creates a user with that as the password (hashed),
# switches mode to "session", writes updated config.
```

### 4. Session Storage (SQLite)

New table in the existing session store:

```sql
CREATE TABLE IF NOT EXISTS auth_sessions (
  id TEXT PRIMARY KEY,
  username TEXT NOT NULL,
  role TEXT NOT NULL,
  refresh_token_hash TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  last_used_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  expires_at TEXT NOT NULL,
  revoked INTEGER NOT NULL DEFAULT 0,
  ip_address TEXT,
  user_agent TEXT
);

CREATE INDEX IF NOT EXISTS idx_auth_sessions_username ON auth_sessions(username);
CREATE INDEX IF NOT EXISTS idx_auth_sessions_expires ON auth_sessions(expires_at);
```

Periodic cleanup: expired and revoked sessions deleted on a schedule (hourly cron or lazy on auth check).

### 5. Webchat Integration

The webchat UI currently stores a static token in `localStorage`. With session auth:

1. **Login page** replaces the token entry form. Username + password fields.
2. **Access token** stored in memory only (not `localStorage` — XSS-safe). Refreshed automatically.
3. **Refresh token** stored as `httpOnly`, `Secure`, `SameSite=Strict` cookie. Not accessible to JavaScript.
4. **SSE connection** authenticates via cookie (refresh token validates the connection, or a short-lived SSE-specific token is issued). Eliminates the `?token=` query param leak.
5. **Auto-refresh:** Before the access token expires, the client calls `/api/auth/refresh`. On failure (revoked, expired refresh), redirect to login.

```typescript
// ui/lib/auth.ts

let accessToken: string | null = null;
let refreshTimer: ReturnType<typeof setTimeout> | null = null;

export function getAccessToken(): string | null {
  return accessToken;
}

export async function login(username: string, password: string): Promise<boolean> {
  const res = await fetch("/api/auth/login", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ username, password }),
    credentials: "include",  // send/receive cookies
  });

  if (!res.ok) return false;

  const data = await res.json();
  accessToken = data.accessToken;
  scheduleRefresh(data.expiresIn);
  return true;
}

async function refresh(): Promise<boolean> {
  const res = await fetch("/api/auth/refresh", {
    method: "POST",
    credentials: "include",  // refresh token in cookie
  });

  if (!res.ok) {
    accessToken = null;
    // Redirect to login
    return false;
  }

  const data = await res.json();
  accessToken = data.accessToken;
  scheduleRefresh(data.expiresIn);
  return true;
}

function scheduleRefresh(expiresInSeconds: number) {
  if (refreshTimer) clearTimeout(refreshTimer);
  // Refresh 60 seconds before expiry
  const ms = Math.max(0, (expiresInSeconds - 60) * 1000);
  refreshTimer = setTimeout(() => refresh(), ms);
}
```

**Backward compatibility:** If `mode: "token"`, the current token entry form stays. The UI detects the mode via `/api/auth/mode` (new endpoint, unauthenticated):

```
GET /api/auth/mode → { mode: "token" | "session", branding: {...} }
```

### 6. TLS

Aletheia should serve HTTPS natively. Two modes:

```yaml
gateway:
  tls:
    enabled: true
    mode: "auto"             # "auto" | "provided"

    # auto mode: self-signed cert generated on first run
    # stored at $ALETHEIA_HOME/credentials/tls/
    autoSubjectAltNames:
      - "192.168.1.100"  # example LAN IP
      - "100.87.6.45"
      - "localhost"

    # provided mode: user supplies cert + key
    certFile: "/path/to/cert.pem"
    keyFile: "/path/to/key.pem"
```

**Auto mode:** On first startup, generate a self-signed certificate using `node:crypto` (X509Certificate API available in Node 20+). Store at `$ALETHEIA_HOME/credentials/tls/server.crt` and `server.key`. Regenerate if expired or if SANs change.

**Hono + @hono/node-server** supports TLS via the underlying `node:https` module:

```typescript
import { createServer } from "node:https";
import { readFileSync } from "node:fs";

const server = createServer({
  cert: readFileSync(certPath),
  key: readFileSync(keyPath),
});

serve({ fetch: app.fetch, createServer: () => server, port });
```

**HTTP redirect:** When TLS is enabled, also bind an HTTP listener on the same port + 80 (or configurable) that redirects all requests to HTTPS. This prevents accidentally connecting over HTTP.

**HSTS header:** Added automatically when TLS is enabled:

```
Strict-Transport-Security: max-age=31536000; includeSubDomains
```

**Tailscale consideration:** Tailscale already provides WireGuard encryption between devices. TLS on top of Tailscale is defense-in-depth — it protects against scenarios where Tailscale is misconfigured, or when accessing via LAN IP instead of Tailscale IP. Worth having.

### 7. Role-Based Access Control

Middleware checks the authenticated user's role against the required permission for each route:

```typescript
// auth/rbac.ts

const ROUTE_PERMISSIONS: Record<string, string> = {
  // Chat
  "POST /api/sessions/send": "api:chat",
  "POST /api/sessions/stream": "api:chat",

  // Read
  "GET /api/sessions": "api:sessions:read",
  "GET /api/sessions/:id/history": "api:sessions:read",
  "GET /api/agents": "api:agents:read",
  "GET /api/agents/:id": "api:agents:read",
  "GET /api/agents/:id/identity": "api:agents:read",
  "GET /api/metrics": "api:metrics:read",
  "GET /api/costs/*": "api:metrics:read",
  "GET /api/events": "api:events",
  "GET /api/branding": "api:branding",
  "GET /api/skills": "api:agents:read",

  // Admin
  "POST /api/sessions/:id/archive": "api:admin",
  "POST /api/sessions/:id/distill": "api:admin",
  "GET /api/cron": "api:admin",
  "POST /api/cron/:id/trigger": "api:admin",
  "GET /api/config": "api:admin",
  "GET /api/turns/active": "api:admin",
  "POST /api/turns/:id/abort": "api:admin",
  "GET /api/contacts/pending": "api:admin",
  "POST /api/contacts/:code/approve": "api:admin",
  "POST /api/contacts/:code/deny": "api:admin",
  "GET /api/export/*": "api:admin",
  "GET /api/blackboard": "api:admin",
  "POST /api/blackboard": "api:admin",
  "GET /api/memory/*": "api:admin",
  "POST /api/memory/*": "api:admin",
};
```

The `admin` role has `"*"` — matches everything. The `user` role has explicit permissions. This is checked once in middleware, not scattered across route handlers.

For the `token` auth mode, the token implicitly has the `admin` role (backward compatible — existing behavior is full access).

### 8. Audit Logging

Every authenticated API call gets a structured log entry:

```typescript
interface AuditEntry {
  timestamp: string;
  actor: string;           // username or "token:<truncated>" for legacy mode
  role: string;
  action: string;          // "POST /api/sessions/send"
  target?: string;         // agentId, sessionId, etc.
  ip: string;
  userAgent?: string;
  status: number;          // HTTP response status
  durationMs: number;
}
```

**Storage:** Written to the structured logger (same as existing runtime logs) with a dedicated `audit` category. Also optionally written to a dedicated SQLite table for queryable history:

```sql
CREATE TABLE IF NOT EXISTS audit_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  actor TEXT NOT NULL,
  role TEXT NOT NULL,
  action TEXT NOT NULL,
  target TEXT,
  ip TEXT,
  user_agent TEXT,
  status INTEGER NOT NULL,
  duration_ms INTEGER
);

CREATE INDEX IF NOT EXISTS idx_audit_actor ON audit_log(actor);
CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp);
```

**API:** `GET /api/audit?actor=&since=&until=&limit=` (admin only).

### 9. Security Headers Hardening

Update the global middleware:

```typescript
app.use("*", async (c, next) => {
  // Existing
  c.header("X-Content-Type-Options", "nosniff");
  c.header("X-Frame-Options", "DENY");
  c.header("Referrer-Policy", "no-referrer");
  c.header("X-XSS-Protection", "0");

  // New
  c.header("Content-Security-Policy",
    "default-src 'self'; " +
    "script-src 'self'; " +
    "style-src 'self' 'unsafe-inline'; " +   // Svelte uses inline styles
    "img-src 'self' data:; " +
    "connect-src 'self'; " +
    "font-src 'self'; " +
    "object-src 'none'; " +
    "frame-ancestors 'none'; " +
    "base-uri 'self'; " +
    "form-action 'self'"
  );
  c.header("Permissions-Policy",
    "camera=(), microphone=(), geolocation=(), payment=()"
  );

  // HSTS — only when TLS is enabled
  if (tlsEnabled) {
    c.header("Strict-Transport-Security", "max-age=31536000; includeSubDomains");
  }

  return next();
});
```

### 10. Rate Limiting — All Endpoints

Extend the existing per-IP rate limiter from MCP-only to all API routes:

```yaml
gateway:
  rateLimit:
    requestsPerMinute: 60        # general API
    authRequestsPerMinute: 10    # login/refresh — tighter to prevent brute force
    streamRequestsPerMinute: 20  # streaming endpoints (expensive)
```

```typescript
// Separate buckets for different endpoint classes
const rateLimiters = {
  auth: createRateLimiter(config.gateway.rateLimit.authRequestsPerMinute),
  stream: createRateLimiter(config.gateway.rateLimit.streamRequestsPerMinute),
  general: createRateLimiter(config.gateway.rateLimit.requestsPerMinute),
};

app.use("/api/auth/*", rateLimiters.auth);
app.use("/api/sessions/stream", rateLimiters.stream);
app.use("/api/*", rateLimiters.general);
```

**Auth rate limiting:** After 5 failed login attempts from the same IP within 15 minutes, enforce exponential backoff (doubling delay up to 5 min). This is separate from the per-minute rate limit — it specifically tracks failed auth attempts.

### 11. Input Validation Hardening

The current code does basic type checks. Formalize:

- **Message length:** Cap `message` field at `maxBodyBytes` (existing, 1MB default). Consider a lower default for chat messages (e.g., 100KB).
- **Agent ID validation:** Whitelist check against `config.agents.list` — already done in most places, but not consistently.
- **Session key validation:** Alphanumeric + `:` + `-` + `_`, max 128 chars. Prevent injection via session keys.
- **SQL injection:** Not a risk (using parameterized queries via better-sqlite3 `.prepare()` consistently), but worth noting in the spec as verified.
- **Path traversal:** The UI file serving already checks `fullPath.startsWith(distDir)` — good. The MCP and API don't serve files.

### 12. MCP Token Improvements

The existing MCP token system is decent. Improvements:

- **Token rotation:** Add `aletheia mcp rotate-token <name>` CLI command.
- **Expiry:** Optional `expiresAt` field per token.
- **Usage logging:** Log MCP tool calls to audit log with client name.
- **Connection to main auth:** In `session` mode, MCP tokens could optionally be scoped session tokens instead of static files. Defer this — static tokens are fine for machine-to-machine.

---

## File Structure

```
src/auth/
  tokens.ts            # JWT signing/verification (HMAC-SHA256)
  passwords.ts         # Argon2id hashing/verification
  sessions.ts          # Session CRUD (SQLite)
  middleware.ts         # Auth middleware (multi-mode: token, password, session)
  rbac.ts              # Role definitions, permission checking
  audit.ts             # Audit log writer
  tls.ts               # Certificate generation, TLS server setup
```

---

## Migration Path

### Phase 1: TLS + Security Headers
1. Add `gateway.tls` config section to schema
2. Implement auto-cert generation (`auth/tls.ts`)
3. Modify `startGateway()` to use HTTPS when TLS enabled
4. HTTP → HTTPS redirect listener
5. Add CSP, Permissions-Policy, HSTS headers
6. Add rate limiting to all API endpoints (not just MCP)
7. **Zero breaking changes** — TLS is opt-in, headers are additive

### Phase 2: Session Auth + RBAC
1. Add `gateway.auth.session`, `users`, `roles` to schema
2. Implement password hashing (`auth/passwords.ts`)
3. Implement JWT signing (`auth/tokens.ts`)
4. Implement session storage (`auth/sessions.ts`)
5. Add auth endpoints: `/api/auth/login`, `/api/auth/refresh`, `/api/auth/logout`, `/api/auth/me`, `/api/auth/mode`
6. Implement multi-mode auth middleware (supports `token`, `password`, and `session` simultaneously during migration)
7. Implement RBAC middleware (`auth/rbac.ts`)
8. **Backward compatible** — `mode: "token"` behavior unchanged

### Phase 3: Webchat Integration
1. Update UI login flow: detect mode via `/api/auth/mode`
2. Session mode: username/password form → access token in memory + refresh token in cookie
3. Token mode: existing token entry form (unchanged)
4. SSE authentication via cookie instead of query param
5. Auto-refresh logic in UI
6. Logout button in UI

### Phase 4: Audit Logging
1. Implement audit middleware (`auth/audit.ts`)
2. Add `audit_log` table to schema
3. Add `GET /api/audit` admin endpoint
4. Integrate with structured logger

### Phase 5: CLI Tooling
1. `aletheia auth hash-password` — interactive password hashing
2. `aletheia auth add-user` — add user to config
3. `aletheia auth migrate` — migrate from token mode to session mode
4. `aletheia auth sessions` — list active sessions
5. `aletheia auth revoke` — revoke a session or all sessions for a user
6. `aletheia mcp rotate-token` — rotate MCP token

---

## Config Schema Changes

```typescript
// taxis/schema.ts additions

const TlsConfig = z.object({
  enabled: z.boolean().default(false),
  mode: z.enum(["auto", "provided"]).default("auto"),
  autoSubjectAltNames: z.array(z.string()).default(["localhost"]),
  certFile: z.string().optional(),
  keyFile: z.string().optional(),
}).default({});

const AuthUserConfig = z.object({
  username: z.string(),
  passwordHash: z.string(),
  role: z.string().default("user"),
});

const SessionAuthConfig = z.object({
  secret: z.string().optional(),          // auto-generated if absent
  accessTokenTtl: z.number().default(900),
  refreshTokenTtl: z.number().default(604800),
  maxSessions: z.number().default(10),
  secureCookies: z.boolean().default(true),
}).default({});

const RolesConfig = z.record(
  z.string(),
  z.array(z.string()),
).default({
  admin: ["*"],
  user: ["api:chat", "api:sessions:read", "api:agents:read", "api:metrics:read", "api:events", "api:branding"],
  readonly: ["api:sessions:read", "api:agents:read", "api:metrics:read", "api:branding"],
});

// Updated GatewayConfig
const GatewayConfig = z.object({
  port: z.number().default(18789),
  bind: z.enum(["auto", "lan", "loopback", "custom"]).default("lan"),
  tls: TlsConfig,
  auth: z.object({
    mode: z.enum(["token", "password", "session"]).default("token"),
    token: z.string().optional(),
    session: SessionAuthConfig,
    users: z.array(AuthUserConfig).default([]),
    roles: RolesConfig,
  }).default({}),
  controlUi: z.object({
    enabled: z.boolean().default(true),
    allowInsecureAuth: z.boolean().default(false),
  }).default({}),
  mcp: z.object({
    requireAuth: z.boolean().default(true),
  }).default({}),
  rateLimit: z.object({
    requestsPerMinute: z.number().default(60),
    authRequestsPerMinute: z.number().default(10),
    streamRequestsPerMinute: z.number().default(20),
  }).default({}),
  cors: z.object({
    allowOrigins: z.array(z.string()).default([]),
  }).default({}),
  maxBodyBytes: z.number().default(1_048_576),
}).passthrough().default({});
```

---

## Security Model Summary

| Layer | Current | After |
|-------|---------|-------|
| **Transport** | HTTP (plaintext) | HTTPS (TLS, self-signed or provided) |
| **Auth** | Static bearer token, never expires | Session-based JWT with refresh, configurable TTL |
| **Storage** | Token in localStorage (XSS-vulnerable) | Access token in memory, refresh token in httpOnly cookie |
| **SSE auth** | Token in URL query param (leaks in logs) | Cookie-based auth (no query params) |
| **Authorization** | All-or-nothing (have token = full access) | Role-based (admin/user/readonly) |
| **Password** | Plaintext in config | Argon2id hash in config |
| **Revocation** | Impossible (restart to change token) | Per-session revocation, per-user revoke-all |
| **Audit** | None | Structured audit log per request |
| **Rate limiting** | MCP only, per-IP | All endpoints, per-IP, auth-specific brute-force protection |
| **Headers** | Basic (XFO, XCTO, Referrer) | Full (+ CSP, HSTS, Permissions-Policy) |
| **MCP** | Static file tokens | Static tokens + optional expiry + rotation CLI |

---

## Metrics

Success looks like:

- `mode: "session"` works end-to-end: login → chat → refresh → logout
- TLS auto-cert generates valid self-signed cert on first run
- Existing `mode: "token"` deployments work with zero config changes
- Access token in browser memory, not localStorage or URL
- Failed login attempts are rate-limited and logged
- Admin endpoints return 403 for non-admin users
- `aletheia doctor` validates auth config (missing hashes, weak passwords, etc.)
- Audit log captures who did what and when
