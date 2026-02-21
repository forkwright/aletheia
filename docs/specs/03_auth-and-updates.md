# Spec: Login Authentication & Self-Update System

**Status:** Phase 1 done (PR #50), Part 2 Phase 1b done (PR #70)
**Author:** Syn  
**Date:** 2026-02-19  

---

## Problem

### Authentication

The webchat gateway uses a static bearer token for authentication. This token lives in `aletheia.json`, is stored in `localStorage` on the client, and is passed via URL query params for SSE connections (visible in browser DevTools, logs, and history). It never expires, can't be revoked per-device, and there's no concept of "logging in" — you either have the token or you don't.

Every other self-hosted tool Aletheia is modeled after — Grafana, Home Assistant, Portainer, Gitea — uses a login page with username/password, session cookies, and "remember me." Aletheia should work the same way.

The auth module from PR #26 already implements most of the backend: scrypt password hashing (`auth/passwords.ts`), JWT access tokens with HMAC-SHA256 (`auth/tokens.ts`), refresh token rotation with SQLite-backed sessions (`auth/sessions.ts`), multi-mode middleware (`auth/middleware.ts`), and RBAC (`auth/rbac.ts`). **None of this is wired into the gateway.** The gateway still uses the old `mode: "token"` path. This spec connects the existing pieces and adds the missing frontend.

### Updates

Deploying changes requires: `git pull`, `npm ci` (if deps changed), `npm run build`, `systemctl restart aletheia`, and if migrations changed, hoping they run cleanly on startup. Claude Code currently babysits this process. There are no tagged releases, no version tracking, no rollback capability, and no way for the operator to know an update is available without checking git.

---

## Part 1: Login Authentication

### Design

#### What changes for the user

1. First visit to webchat → login page (username + password + "remember me" checkbox)
2. Successful login → redirected to chat, session persists via httpOnly cookie
3. "Remember me" checked → session lasts 30 days; unchecked → session lasts until browser closes
4. Multiple devices can be logged in simultaneously (capped at 10 sessions per user)
5. `/settings` page shows active sessions with device/IP, allows revoking any session
6. API access (MCP, programmatic) still uses bearer tokens — these become "personal access tokens" generated from the settings page

#### What changes for first setup

The `aletheia setup` wizard (from the onboarding spec) creates the admin account:

```
Welcome to Aletheia.

Create your admin account:
  Username: cody
  Password: ********
  Confirm:  ********

Admin account created.
```

This writes the password hash to `aletheia.json` under the new schema:

```json
{
  "gateway": {
    "auth": {
      "mode": "session",
      "users": [
        {
          "username": "cody",
          "passwordHash": "$scrypt$N=16384,r=8,p=1$...$...",
          "role": "admin"
        }
      ],
      "session": {
        "accessTokenTtl": 900,
        "refreshTokenTtl": 2592000,
        "maxSessionsPerUser": 10,
        "secureCookies": true
      }
    }
  }
}
```

For existing installs, `aletheia migrate-auth` converts from token mode:

```
Current auth mode: token
Migrating to session-based auth.

Create your admin account:
  Username: cody
  Password: ********

Auth migrated. Old token preserved as personal access token.
Restart the gateway to apply: systemctl restart aletheia
```

### Schema Changes

**File:** `infrastructure/runtime/src/taxis/schema.ts`

Expand the gateway auth schema to support session mode:

```typescript
auth: z.object({
  mode: z.enum(["none", "token", "session"]).default("session"),
  // Legacy: static bearer token (kept for API/programmatic access)
  token: z.string().optional(),
  // Session mode: user accounts with password auth
  users: z.array(z.object({
    username: z.string(),
    passwordHash: z.string(),
    role: z.enum(["admin", "user", "readonly"]).default("admin"),
  })).default([]),
  session: z.object({
    // JWT access token lifetime (seconds). Short-lived, auto-refreshed.
    accessTokenTtl: z.number().default(900),        // 15 minutes
    // Refresh token lifetime (seconds). "Remember me" uses this full duration.
    // Without "remember me", the cookie is session-scoped (browser close = logout).
    refreshTokenTtl: z.number().default(2_592_000), // 30 days
    // Max concurrent sessions per user.
    maxSessionsPerUser: z.number().default(10),
    // Set Secure flag on cookies. Should be true unless testing over plain HTTP.
    secureCookies: z.boolean().default(true),
  }).default({}),
}).default({}),
```

### Gateway Routes

**File:** `infrastructure/runtime/src/pylon/server.ts`

Wire the existing `createAuthMiddleware` and `createAuthRoutes` from `auth/middleware.ts`:

```
POST /api/auth/login        — { username, password, rememberMe }
                              → Set-Cookie: aletheia_refresh=<token>; HttpOnly; SameSite=Strict; [Secure]
                              → { accessToken, expiresIn, username, role }

POST /api/auth/refresh      — Cookie: aletheia_refresh=<token>
                              → Set-Cookie: aletheia_refresh=<newToken> (rotation)
                              → { accessToken, expiresIn }

POST /api/auth/logout       — Cookie: aletheia_refresh=<token>
                              → Clear-Cookie: aletheia_refresh
                              → { ok: true }

GET  /api/auth/mode         — { mode, sessionAuth }  (public, no auth required)

GET  /api/auth/sessions     — List active sessions for current user
DELETE /api/auth/sessions/:id — Revoke a specific session
```

**Cookie details:**
- `aletheia_refresh` — httpOnly, SameSite=Strict, Secure (configurable), Path=/api/auth
- When "remember me" is checked: `Max-Age=<refreshTokenTtl>` (30 days default)
- When "remember me" is unchecked: no Max-Age (session cookie — cleared on browser close)
- The access token is *not* stored in a cookie — it's held in memory by the JS client and sent as a Bearer header. This avoids CSRF entirely.

**Auth flow:**
1. Client calls `GET /api/auth/mode` to determine auth type
2. If `mode === "session"`: show login form
3. On login: `POST /api/auth/login` → server sets refresh cookie, returns access token
4. Client stores access token in memory (not localStorage), sends as `Authorization: Bearer` on all API calls
5. When access token expires (401 response): client calls `POST /api/auth/refresh` → gets new access token
6. Refresh token rotation: each refresh call issues a new refresh token and invalidates the old one

**SSE connection auth:** The current SSE endpoint passes the token as a query param because `EventSource` doesn't support custom headers. With session auth, use the refresh cookie instead — SSE requests include cookies automatically. The server validates the cookie and creates a short-lived internal token for the SSE stream. Alternatively, the client can use `fetch()` with `ReadableStream` instead of `EventSource`, which supports headers.

**Backward compatibility:**
- `mode: "token"` continues to work exactly as today (static bearer token)
- `mode: "session"` is the new default for fresh installs
- `mode: "none"` disables auth (dev/testing only)
- If `mode: "session"` but no users are configured, the gateway starts in "setup required" mode — all routes redirect to a one-time account creation page

### Frontend Changes

**Login page** (`ui/src/routes/Login.svelte`):
- Username field, password field, "Remember me" checkbox, submit button
- Error display for bad credentials
- Redirect to chat on success
- Shown when `GET /api/auth/mode` returns `sessionAuth: true` and no valid access token exists

**Auth client** (`ui/src/lib/auth.ts`):
- `login(username, password, rememberMe)` → calls API, stores access token in memory
- `refresh()` → called automatically on 401, uses refresh cookie
- `logout()` → calls API, clears in-memory token
- `getAccessToken()` → returns current token or triggers refresh
- `isAuthenticated()` → true if access token is valid and not expired
- Wraps `fetch()` to automatically inject Bearer header and handle 401 → refresh → retry

**Session management** (in settings page):
- List active sessions: device, IP, last used, created
- "Revoke" button per session
- "Revoke all other sessions" button

### Migration Path

For existing installs (currently on `mode: "token"`):

1. `aletheia migrate-auth` CLI command:
   - Prompts for username/password
   - Hashes password with scrypt
   - Adds user to `aletheia.json` under `gateway.auth.users`
   - Changes `gateway.auth.mode` to `"session"`
   - Preserves old token as a personal access token (still works for API calls)
   - Validates config with `aletheia doctor`
   - Prints restart instructions

2. On next restart, the gateway uses session mode. Existing webchat tabs will get a 401, see the login page, and log in with the new credentials.

---

## Part 2: Self-Update System

### Design

#### Versioning

Aletheia follows semver. The current state is pre-1.0 — all releases are `0.x.y`:

- **0.x.0** — significant feature additions or breaking changes
- **0.x.y** — bug fixes, small features, non-breaking changes

`package.json` already has `"version": "0.9.0"`. This becomes the source of truth. The CLI reports it via `aletheia --version`.

Version 1.0 is reserved for when the system is stable enough for other people to run without hand-holding — post-onboarding spec, post-auth spec, with documentation.

#### GitHub Releases

Each release is a GitHub Release created from a git tag:

```
Tag: v0.9.1
Title: v0.9.1 — Turn Safety & Data Privacy
Body: (auto-generated changelog from commits since last tag)
```

**Release workflow** (`.github/workflows/release.yml`):

```yaml
name: Release

on:
  push:
    tags: ["v*"]

permissions:
  contents: write

jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
          cache-dependency-path: infrastructure/runtime/package-lock.json

      - run: cd infrastructure/runtime && npm ci
      - run: cd infrastructure/runtime && npm run typecheck
      - run: cd infrastructure/runtime && npm run build

      - name: Generate changelog
        id: changelog
        run: |
          PREV_TAG=$(git describe --tags --abbrev=0 HEAD^ 2>/dev/null || echo "")
          if [ -n "$PREV_TAG" ]; then
            CHANGES=$(git log --oneline --no-merges ${PREV_TAG}..HEAD | head -50)
          else
            CHANGES=$(git log --oneline --no-merges -20)
          fi
          echo "CHANGES<<EOF" >> $GITHUB_OUTPUT
          echo "$CHANGES" >> $GITHUB_OUTPUT
          echo "EOF" >> $GITHUB_OUTPUT

      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          generate_release_notes: true
          body: |
            ## Changes

            ${{ steps.changelog.outputs.CHANGES }}

            ## Install / Update

            ```bash
            aletheia update
            ```
```

**Cutting a release:**

```bash
# Bump version in package.json, commit, tag, push
cd infrastructure/runtime
npm version patch    # or minor, or major
cd ../..
git push && git push --tags
```

The `npm version` command updates `package.json`, creates a commit, and creates the git tag. The push triggers the release workflow.

#### Update CLI

**Command:** `aletheia update [version] [--edge] [--check] [--rollback]`

```
aletheia update              # Update to latest release
aletheia update v0.9.2       # Update to specific version
aletheia update --edge       # Update to latest main (HEAD), regardless of release
aletheia update --check      # Check for updates without applying
aletheia update --rollback   # Roll back to previous version
```

**Implementation:** `shared/bin/aletheia-update` (standalone bash script, not part of the runtime — because it needs to survive the runtime being stopped)

```bash
#!/usr/bin/env bash
set -euo pipefail

ALETHEIA_ROOT="${ALETHEIA_ROOT:-/mnt/ssd/aletheia}"
RUNTIME_DIR="$ALETHEIA_ROOT/infrastructure/runtime"
SERVICE_NAME="aletheia"
BACKUP_DIR="$ALETHEIA_ROOT/.update-backups"
LOCKFILE="/tmp/aletheia-update.lock"

# --- Version resolution ---

resolve_target() {
  local target="$1"
  local edge="$2"

  if [ "$edge" = "true" ]; then
    echo "edge"  # will pull HEAD of main
    return
  fi

  if [ -n "$target" ]; then
    # Specific version requested
    if ! git -C "$ALETHEIA_ROOT" rev-parse "refs/tags/$target" &>/dev/null; then
      echo "ERROR: Tag $target not found" >&2
      return 1
    fi
    echo "$target"
    return
  fi

  # Latest release tag
  local latest
  latest=$(git -C "$ALETHEIA_ROOT" tag -l 'v*' --sort=-v:refname | head -1)
  if [ -z "$latest" ]; then
    echo "ERROR: No release tags found. Use --edge for latest main." >&2
    return 1
  fi
  echo "$latest"
}

# --- Pre-flight checks ---

preflight() {
  # 1. Check we're on main branch
  local branch
  branch=$(git -C "$ALETHEIA_ROOT" branch --show-current)
  if [ "$branch" != "main" ]; then
    echo "ERROR: Not on main branch (on '$branch'). Aborting." >&2
    return 1
  fi

  # 2. Check for uncommitted changes
  if ! git -C "$ALETHEIA_ROOT" diff --quiet; then
    echo "ERROR: Uncommitted changes in working tree. Commit or stash first." >&2
    return 1
  fi

  # 3. Check git remote is reachable
  if ! git -C "$ALETHEIA_ROOT" fetch --tags --quiet 2>/dev/null; then
    echo "ERROR: Cannot reach git remote. Check network." >&2
    return 1
  fi
}

# --- Core update ---

do_update() {
  local target="$1"
  local current_version current_commit

  current_version=$(node -e "console.log(require('$RUNTIME_DIR/package.json').version)" 2>/dev/null || echo "unknown")
  current_commit=$(git -C "$ALETHEIA_ROOT" rev-parse --short HEAD)

  echo "Current version: $current_version ($current_commit)"

  # Backup current state
  mkdir -p "$BACKUP_DIR"
  local backup_name="pre-update-$(date +%Y%m%d-%H%M%S)-$current_commit"
  echo "$current_commit" > "$BACKUP_DIR/$backup_name.ref"
  cp "$RUNTIME_DIR/package.json" "$BACKUP_DIR/$backup_name.package.json"

  # Pull changes
  if [ "$target" = "edge" ]; then
    echo "Pulling latest main (edge)..."
    git -C "$ALETHEIA_ROOT" pull --ff-only origin main
  else
    echo "Checking out $target..."
    git -C "$ALETHEIA_ROOT" fetch --tags
    git -C "$ALETHEIA_ROOT" checkout "$target"
  fi

  local new_version
  new_version=$(node -e "console.log(require('$RUNTIME_DIR/package.json').version)" 2>/dev/null || echo "unknown")
  local new_commit
  new_commit=$(git -C "$ALETHEIA_ROOT" rev-parse --short HEAD)
  echo "Target version: $new_version ($new_commit)"

  if [ "$current_commit" = "$new_commit" ]; then
    echo "Already up to date."
    return 0
  fi

  # Install dependencies (only if lockfile changed)
  if ! git -C "$ALETHEIA_ROOT" diff --quiet "$current_commit" "$new_commit" -- infrastructure/runtime/package-lock.json; then
    echo "Dependencies changed — running npm ci..."
    (cd "$RUNTIME_DIR" && npm ci --prefer-offline)
  else
    echo "Dependencies unchanged — skipping npm ci."
  fi

  # Build
  echo "Building..."
  if ! (cd "$RUNTIME_DIR" && npm run build); then
    echo "ERROR: Build failed. Rolling back..."
    git -C "$ALETHEIA_ROOT" checkout "$current_commit"
    return 1
  fi

  # Restart service
  echo "Restarting $SERVICE_NAME..."
  sudo systemctl restart "$SERVICE_NAME"

  # Health check (wait up to 30s)
  echo "Waiting for health check..."
  local attempts=0
  local healthy=false
  while [ $attempts -lt 15 ]; do
    sleep 2
    if curl -sf http://localhost:18789/health > /dev/null 2>&1; then
      healthy=true
      break
    fi
    attempts=$((attempts + 1))
  done

  if [ "$healthy" = "true" ]; then
    echo ""
    echo "✓ Updated: $current_version ($current_commit) → $new_version ($new_commit)"

    # Record successful update
    echo "$new_commit $new_version $(date -Iseconds)" >> "$BACKUP_DIR/history.log"
  else
    echo "ERROR: Health check failed after update. Rolling back..."
    git -C "$ALETHEIA_ROOT" checkout "$current_commit"
    (cd "$RUNTIME_DIR" && npm run build)
    sudo systemctl restart "$SERVICE_NAME"
    echo "Rolled back to $current_version ($current_commit)"
    return 1
  fi
}
```

**Key behaviors:**

1. **Pre-flight checks** — must be on `main`, no uncommitted changes, remote reachable
2. **Backup before update** — records the current commit hash so rollback is always possible
3. **Conditional npm ci** — only runs if `package-lock.json` actually changed between versions. Most updates skip this entirely, saving 30+ seconds.
4. **Build before restart** — builds the new code while the old version is still serving. Only restarts once the build succeeds. If build fails, rolls back the git checkout and the old version keeps running.
5. **Health check after restart** — polls `/health` for up to 30 seconds. If the new version doesn't come up healthy, automatically rolls back to the previous commit, rebuilds, and restarts.
6. **`--edge` flag** — pulls `HEAD` of main instead of a release tag. For development/testing. Skips tag resolution entirely.
7. **Rollback** — `aletheia update --rollback` reads the last backup ref and checks out that commit.

#### Update Check

**File:** `infrastructure/runtime/src/daemon/update-check.ts`

A lightweight daemon that runs on startup and every 6 hours. Checks the GitHub API for the latest release tag and compares against the local version. Stores the result on the blackboard so the UI can display it.

```typescript
import { createLogger } from "../koina/logger.js";
import type { SessionStore } from "../mneme/store.js";

const log = createLogger("daemon:update-check");

const REPO = "forkwright/aletheia";
const CHECK_INTERVAL = 6 * 60 * 60 * 1000; // 6 hours

interface UpdateInfo {
  available: boolean;
  currentVersion: string;
  latestVersion: string;
  latestTag: string;
  releaseUrl: string;
  checkedAt: string;
}

export function startUpdateChecker(
  store: SessionStore,
  currentVersion: string,
): NodeJS.Timeout {
  const check = async () => {
    try {
      const res = await fetch(
        `https://api.github.com/repos/${REPO}/releases/latest`,
        { signal: AbortSignal.timeout(10_000) },
      );
      if (!res.ok) return;

      const release = await res.json() as {
        tag_name: string;
        html_url: string;
      };

      const latestVersion = release.tag_name.replace(/^v/, "");
      const available = isNewer(latestVersion, currentVersion);

      const info: UpdateInfo = {
        available,
        currentVersion,
        latestVersion,
        latestTag: release.tag_name,
        releaseUrl: release.html_url,
        checkedAt: new Date().toISOString(),
      };

      store.blackboardWrite("system:update", JSON.stringify(info), 7 * 24 * 3600);

      if (available) {
        log.info(`Update available: ${currentVersion} → ${latestVersion}`);
      }
    } catch (err) {
      log.debug(`Update check failed: ${err instanceof Error ? err.message : err}`);
    }
  };

  // Initial check 60s after startup (don't block boot)
  setTimeout(check, 60_000);
  return setInterval(check, CHECK_INTERVAL);
}

function isNewer(latest: string, current: string): boolean {
  const [lMajor, lMinor, lPatch] = latest.split(".").map(Number);
  const [cMajor, cMinor, cPatch] = current.split(".").map(Number);
  if ((lMajor ?? 0) !== (cMajor ?? 0)) return (lMajor ?? 0) > (cMajor ?? 0);
  if ((lMinor ?? 0) !== (cMinor ?? 0)) return (lMinor ?? 0) > (cMinor ?? 0);
  return (lPatch ?? 0) > (cPatch ?? 0);
}
```

#### UI Notification

The webchat header shows a subtle update indicator when `system:update` blackboard entry has `available: true`:

```
🔄 Update available: v0.9.2 — run `aletheia update` to install
```

Not a modal, not a blocking banner — just a small notification in the header/settings area. Clicking it shows the release notes (links to GitHub).

Future enhancement: an "Update now" button in the UI that calls a protected API endpoint (`POST /api/system/update`), which spawns the update script as a detached process. The endpoint returns immediately with a status URL, and the UI polls for completion. This is Phase 2 — the CLI is Phase 1.

#### API Endpoint (Phase 2)

```
POST /api/system/update        — { version?: string, edge?: boolean }
                                 Requires admin role.
                                 Spawns aletheia-update as detached process.
                                 → { status: "started", logFile: "/tmp/aletheia-update.log" }

GET  /api/system/update/status — Reads the update log file.
                                 → { status: "running" | "success" | "failed" | "rolled_back", log: "..." }
```

The update script writes structured progress to a log file. The API reads that file. The runtime doesn't need to survive the update — the detached script handles restart and health check independently.

### GitHub Releases Setup

**To set up releases on the repo:**

1. The release workflow (`.github/workflows/release.yml` above) is all that's needed on the GitHub side. No repo settings changes required — the `softprops/action-gh-release` action creates releases automatically when a tag is pushed.

2. **First release:**

```bash
cd /mnt/ssd/aletheia/infrastructure/runtime

# Set the version (already 0.9.0 — bump to 0.9.1 for first release)
npm version patch -m "release: v%s"

cd ../..
git push && git push --tags
```

This creates tag `v0.9.1`, pushes it, and the workflow creates the GitHub Release with auto-generated changelog.

3. **Subsequent releases:**

```bash
# For bug fixes:
cd infrastructure/runtime && npm version patch -m "release: v%s" && cd ../.. && git push && git push --tags

# For features:
cd infrastructure/runtime && npm version minor -m "release: v%s" && cd ../.. && git push && git push --tags
```

4. **No v1.0 yet.** The `0.x` series signals "this works but the API/config surface may change." v1.0 should mean: setup wizard works, auth is solid, updates are seamless, documentation exists for someone who isn't us.

---

## Implementation Order

| Phase | What | Effort | Dependencies |
|-------|------|--------|--------------|
| **1a** | Release workflow + first tag | Small | None |
| **1b** ✅ | `aletheia-update` CLI script | Medium | 1a (needs tags to exist) |
| **1c** | Update check daemon + blackboard | Small | 1a |
| **2a** | Gateway auth schema expansion | Small | None |
| **2b** | Wire existing auth modules into gateway | Medium | 2a |
| **2c** | Login page + auth client in webchat UI | Medium | 2b |
| **2d** | `aletheia migrate-auth` CLI command | Small | 2b |
| **2e** | SSE auth migration (cookie-based) | Small | 2b, 2c |
| **3a** | Session management UI (settings page) | Small | 2c |
| **3b** | Update button in UI (calls API) | Small | 1b, 2c |
| **3c** | `POST /api/system/update` endpoint | Medium | 1b, 2b |
| **4a** | Auth credential failover (F-23) | Small | 2b — fallback credentials on 429/5xx from LLM providers |

**Recommended start:** 1a → 1b → 1c (get updates working first, it's the bigger daily pain point), then 2a → 2b → 2c → 2d (auth).

---

## Testing

### Auth

- **Login flow:** Create user, POST /api/auth/login with correct password → 200 + access token + refresh cookie. Bad password → 401.
- **Token refresh:** Use refresh cookie to get new access token. Old refresh token becomes invalid (rotation).
- **Session expiry:** Access token with short TTL → 401 after expiry → refresh succeeds → new access token.
- **Remember me:** With rememberMe=true, cookie has Max-Age. Without, cookie is session-scoped.
- **Concurrent sessions:** Log in from 10 devices → all work. Log in from 11th → oldest session evicted.
- **Revocation:** Revoke session → that device's refresh token stops working.
- **Migration:** Run `aletheia migrate-auth` on a token-mode config → config updated to session mode, old token preserved.
- **Backward compat:** Set `mode: "token"` → old bearer token flow works exactly as before.

### Updates

- **Check:** `aletheia update --check` when behind → reports available update. When current → reports up to date.
- **Update to release:** Tag a test release, `aletheia update` → pulls tag, builds, restarts, health check passes.
- **Update edge:** `aletheia update --edge` → pulls HEAD of main regardless of tags.
- **Update specific version:** `aletheia update v0.9.1` → checks out that exact tag.
- **Build failure:** Corrupt a source file, attempt update → build fails, git rolls back, old version keeps running.
- **Health check failure:** Deploy a version that crashes on startup → health check fails, auto-rollback.
- **Rollback:** After successful update, `aletheia update --rollback` → returns to previous version.
- **Skip npm ci:** Update where only `.ts` files changed → no `npm ci` run.
- **Uncommitted changes:** `aletheia update` with dirty working tree → refuses to run.

---

## Success Criteria

- **Auth:** No more static tokens in localStorage or URL params. Login page works. "Remember me" works. Multiple devices work. Existing token-mode installs can migrate in one command.
- **Updates:** `aletheia update` takes <60 seconds for a typical update (no dep changes). Automatic rollback works. The operator never has to manually run `git pull && npm ci && npm run build && systemctl restart`.
- **Releases:** Tagged releases on GitHub with changelogs. Version visible in `aletheia --version` and webchat UI footer.
