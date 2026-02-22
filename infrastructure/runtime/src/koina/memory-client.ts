// Canonical sidecar URL and user ID for memory service access.
// Single source of truth — every module that talks to the memory sidecar imports from here.
// Lazy reads: env vars may be set by taxis config after module import.

export const getSidecarUrl = (): string =>
  process.env["ALETHEIA_MEMORY_URL"] ?? "http://127.0.0.1:8230";

export const getUserId = (): string =>
  process.env["ALETHEIA_MEMORY_USER"] ?? "default";
