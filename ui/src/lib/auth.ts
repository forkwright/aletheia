// Session auth — access token in memory, refresh token in httpOnly cookie
let accessToken: string | null = null;
let refreshTimer: ReturnType<typeof setTimeout> | null = null;
let onAuthFailure: (() => void) | null = null;

export interface AuthMode {
  mode: "token" | "session";
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
  return { ok: true, role: data.role };
}

export async function refresh(): Promise<boolean> {
  try {
    const res = await fetch("/api/auth/refresh", {
      method: "POST",
      credentials: "include",
    });

    if (!res.ok) {
      accessToken = null;
      onAuthFailure?.();
      return false;
    }

    const data = await res.json();
    accessToken = data.accessToken;
    scheduleRefresh(data.expiresIn);
    return true;
  } catch {
    accessToken = null;
    onAuthFailure?.();
    return false;
  }
}

export async function logout(): Promise<void> {
  if (refreshTimer) clearTimeout(refreshTimer);
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
}

function scheduleRefresh(expiresInSeconds: number): void {
  if (refreshTimer) clearTimeout(refreshTimer);
  const ms = Math.max(0, (expiresInSeconds - 60) * 1000);
  refreshTimer = setTimeout(() => {
    refresh();
  }, ms);
}
