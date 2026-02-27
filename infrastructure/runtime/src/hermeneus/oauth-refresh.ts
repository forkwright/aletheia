// OAuth token refresh for Anthropic Max/Pro credentials
// Handles automatic token refresh when access tokens expire (~12h lifetime).
// Refresh tokens have ~30 day lifetime — when those expire, manual re-auth is required.

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";

const log = createLogger("hermeneus.oauth");

/** Known Anthropic OAuth endpoints */
const ANTHROPIC_TOKEN_URL = "https://console.anthropic.com/v1/oauth/token";
const ANTHROPIC_CLIENT_ID = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

export interface OAuthCredentials {
  type: "oauth";
  label?: string;
  token: string;
  refreshToken: string;
  expiresAt: number; // epoch ms
  scopes?: string[];
  subscriptionType?: string;
  backupCredentials?: unknown[];
}

export interface RefreshResult {
  success: boolean;
  newToken?: string;
  newExpiresAt?: number;
  error?: string;
}

/** Buffer before expiry — refresh 5 minutes early to avoid race conditions */
const EXPIRY_BUFFER_MS = 5 * 60 * 1000;

/** Minimum time between refresh attempts to avoid hammering the endpoint */
let lastRefreshAttemptMs = 0;
const MIN_REFRESH_INTERVAL_MS = 30_000; // 30 seconds

/** Reset rate limit state — for testing only */
export function _resetRateLimit(): void {
  lastRefreshAttemptMs = 0;
}

/**
 * Check if a token is expired or about to expire.
 */
export function isTokenExpired(expiresAt: number): boolean {
  return Date.now() >= expiresAt - EXPIRY_BUFFER_MS;
}

/**
 * Read the current credential file and return parsed credentials.
 * Returns null if file doesn't exist or isn't OAuth type.
 */
export function readCredentials(credPath?: string): OAuthCredentials | null {
  const path = credPath ?? defaultCredPath();
  try {
    const raw = JSON.parse(readFileSync(path, "utf-8")) as Record<string, unknown>;
    if (raw["type"] !== "oauth" || typeof raw["token"] !== "string") {
      return null;
    }
    return raw as unknown as OAuthCredentials;
  } catch {
    return null;
  }
}

/**
 * Attempt to refresh an expired OAuth token using the refresh token.
 * On success, updates the credential file on disk and returns the new token.
 * On failure, returns error details — caller should fall through to backup.
 */
export async function refreshOAuthToken(credPath?: string): Promise<RefreshResult> {
  const path = credPath ?? defaultCredPath();

  // Rate-limit refresh attempts
  const now = Date.now();
  if (now - lastRefreshAttemptMs < MIN_REFRESH_INTERVAL_MS) {
    log.warn("OAuth refresh rate-limited — too soon since last attempt");
    return { success: false, error: "Rate limited (< 30s since last attempt)" };
  }
  lastRefreshAttemptMs = now;

  const creds = readCredentials(path);
  if (!creds) {
    log.warn("No OAuth credentials found — cannot refresh");
    return { success: false, error: "No OAuth credentials in credential file" };
  }

  if (!creds.refreshToken) {
    log.warn("No refresh token available — manual re-auth required");
    return { success: false, error: "No refresh token — run 'claude setup-token' to re-authenticate" };
  }

  log.info("Attempting OAuth token refresh...");

  try {
    const response = await fetch(ANTHROPIC_TOKEN_URL, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({
        grant_type: "refresh_token",
        refresh_token: creds.refreshToken,
        client_id: ANTHROPIC_CLIENT_ID,
      }),
    });

    if (!response.ok) {
      const body = await response.text().catch(() => "");
      log.error(`OAuth refresh failed: ${response.status} ${response.statusText} — ${body}`);

      // 400/401 from token endpoint means refresh token is invalid/expired
      if (response.status === 400 || response.status === 401) {
        return {
          success: false,
          error: `Refresh token expired or invalid (${response.status}). Run 'claude setup-token' to re-authenticate.`,
        };
      }
      return {
        success: false,
        error: `Token endpoint returned ${response.status}: ${body.slice(0, 200)}`,
      };
    }

    const data = await response.json() as Record<string, unknown>;
    const newAccessToken = data["access_token"] as string | undefined;
    const newRefreshToken = data["refresh_token"] as string | undefined;
    const expiresIn = data["expires_in"] as number | undefined;

    if (!newAccessToken) {
      log.error("OAuth refresh response missing access_token");
      return { success: false, error: "Response missing access_token field" };
    }

    const newExpiresAt = expiresIn
      ? Date.now() + expiresIn * 1000
      : Date.now() + 12 * 60 * 60 * 1000; // default 12h if not specified

    // Update credential file — preserve all existing fields, update token fields
    const updatedCreds: OAuthCredentials = {
      ...creds,
      token: newAccessToken,
      expiresAt: newExpiresAt,
      // Update refresh token if a new one was issued (rotation)
      ...(newRefreshToken ? { refreshToken: newRefreshToken } : {}),
    };

    writeFileSync(path, JSON.stringify(updatedCreds, null, 2), { mode: 0o600 });
    log.info(
      `OAuth token refreshed successfully — expires ${new Date(newExpiresAt).toISOString()}` +
      (newRefreshToken ? " (refresh token rotated)" : ""),
    );

    return {
      success: true,
      newToken: newAccessToken,
      newExpiresAt,
    };
  } catch (error) {
    const msg = error instanceof Error ? error.message : String(error);
    log.error(`OAuth refresh request failed: ${msg}`);
    return { success: false, error: `Network error: ${msg}` };
  }
}

/**
 * Proactive refresh — call during startup or periodically to refresh
 * tokens before they expire, avoiding mid-request failures.
 */
export async function proactiveRefresh(credPath?: string): Promise<boolean> {
  const creds = readCredentials(credPath ?? defaultCredPath());
  if (!creds) return false;

  if (!isTokenExpired(creds.expiresAt)) {
    const remainingHrs = ((creds.expiresAt - Date.now()) / 3_600_000).toFixed(1);
    log.debug(`OAuth token valid — ${remainingHrs}h remaining`);
    return false;
  }

  log.info("OAuth token expired or expiring soon — proactively refreshing");
  const result = await refreshOAuthToken(credPath);
  return result.success;
}

function defaultCredPath(): string {
  const home = process.env["HOME"] ?? "/tmp";
  return join(home, ".aletheia", "credentials", "anthropic.json");
}
