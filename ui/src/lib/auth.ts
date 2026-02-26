// Session auth — access token in memory, refresh token in httpOnly cookie
let accessToken: string | null = null;
let tokenExpiresAt: number | null = null; // unix ms when access token expires
let refreshTimer: ReturnType<typeof setTimeout> | null = null;
let onAuthFailure: (() => void) | null = null;
let refreshInFlight: Promise<boolean> | null = null;
let visibilityListenerAttached = false;

export interface AuthMode {
  mode: "none" | "token" | "session";
  sessionAuth: boolean;
}

export function getAccessToken(): string | null {
  return accessToken;
}

export function setAccessToken(token: string | null): void {
  accessToken = token;
}

export function setAuthFailureHandler(handler: () => void): void {
  onAuthFailure = handler;
}

export async function fetchAuthMode(): Promise<AuthMode> {
  const res = await fetch("/api/auth/mode");
  if (!res.ok) return { mode: "token", sessionAuth: false };
  return res.json();
}

export async function login(
  username: string,
  password: string,
  rememberMe = true,
): Promise<{ ok: boolean; error?: string; role?: string }> {
  const res = await fetch("/api/auth/login", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ username, password, rememberMe }),
    credentials: "include",
  });

  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: "Login failed" }));
    return { ok: false, error: body.error ?? "Login failed" };
  }

  const data = await res.json();
  accessToken = data.accessToken;
  scheduleRefresh(data.expiresIn);
  attachVisibilityListener();
  return { ok: true, role: data.role };
}

/**
 * Refresh the access token using the httpOnly refresh cookie.
 * Deduplicates concurrent calls — if a refresh is already in flight,
 * callers share the same promise.
 */
export async function refresh(): Promise<boolean> {
  // Deduplicate: if a refresh is already running, piggyback on it
  if (refreshInFlight) return refreshInFlight;

  refreshInFlight = doRefresh();
  try {
    return await refreshInFlight;
  } finally {
    refreshInFlight = null;
  }
}

async function doRefresh(): Promise<boolean> {
  // Retry once with a short delay before giving up
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      const res = await fetch("/api/auth/refresh", {
        method: "POST",
        credentials: "include",
      });

      if (res.ok) {
        const data = await res.json();
        accessToken = data.accessToken;
        scheduleRefresh(data.expiresIn);
        attachVisibilityListener();
        return true;
      }

      // 401 = refresh token invalid/expired — no point retrying
      if (res.status === 401) break;

      // Server error — retry once after short delay
      if (attempt === 0) {
        await sleep(1000);
        continue;
      }
    } catch {
      // Network error — retry once
      if (attempt === 0) {
        await sleep(1000);
        continue;
      }
    }
  }

  // All attempts failed
  accessToken = null;
  tokenExpiresAt = null;
  onAuthFailure?.();
  return false;
}

export async function logout(): Promise<void> {
  teardownRefresh();
  try {
    await fetch("/api/auth/logout", {
      method: "POST",
      headers: accessToken
        ? { Authorization: `Bearer ${accessToken}` }
        : {},
      credentials: "include",
    });
  } catch {
    // best-effort
  }
  accessToken = null;
  tokenExpiresAt = null;
}

/**
 * Check if the access token is expired or about to expire.
 * If so, trigger a refresh proactively.
 * Called on visibility change (tab focus) and window focus.
 */
function checkAndRefreshIfNeeded(): void {
  if (!tokenExpiresAt) return;

  const now = Date.now();
  const margin = 60_000; // 60s before expiry

  if (now >= tokenExpiresAt - margin) {
    // Token expired or about to — refresh immediately
    refresh();
  }
}

function scheduleRefresh(expiresInSeconds: number): void {
  if (refreshTimer) clearTimeout(refreshTimer);

  // Track absolute expiry time so visibility checks work after sleep
  tokenExpiresAt = Date.now() + expiresInSeconds * 1000;

  // Schedule a timer as the primary refresh mechanism.
  // The visibility listener is the safety net for sleep/suspend.
  const ms = Math.max(0, (expiresInSeconds - 60) * 1000);
  refreshTimer = setTimeout(() => {
    refresh();
  }, ms);
}

/**
 * Attach visibility/focus listeners that catch token expiry after sleep.
 * setTimeout pauses when the OS suspends — these events fire on wake.
 */
function attachVisibilityListener(): void {
  if (visibilityListenerAttached) return;
  visibilityListenerAttached = true;

  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") {
      checkAndRefreshIfNeeded();
    }
  });

  window.addEventListener("focus", () => {
    checkAndRefreshIfNeeded();
  });
}

function teardownRefresh(): void {
  if (refreshTimer) {
    clearTimeout(refreshTimer);
    refreshTimer = null;
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
