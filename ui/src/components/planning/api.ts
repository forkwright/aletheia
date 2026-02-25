// Authenticated fetch wrapper for planning components.
// All planning API calls must go through this to include auth headers.

import { getEffectiveToken } from "../../lib/api";

/** Fetch with auth headers. Drop-in replacement for bare fetch(). */
export async function authFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const token = getEffectiveToken();
  const headers: Record<string, string> = {
    ...(token ? { Authorization: `Bearer ${token}` } : {}),
    ...(init?.headers as Record<string, string>),
  };
  // Ensure Content-Type for JSON bodies
  if (init?.body && typeof init.body === "string" && !headers["Content-Type"]) {
    headers["Content-Type"] = "application/json";
  }
  return fetch(input, { ...init, headers });
}
