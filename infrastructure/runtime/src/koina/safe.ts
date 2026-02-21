// Safe execution wrappers — explicit error boundaries for non-critical operations
import { createLogger } from "./logger.js";

const log = createLogger("safe");

export function trySafe<T>(label: string, fn: () => T, fallback: T): T {
  try {
    return fn();
  } catch (err) {
    log.warn(`${label} failed (non-fatal): ${err instanceof Error ? err.message : err}`);
    return fallback;
  }
}

export async function trySafeAsync<T>(label: string, fn: () => Promise<T>, fallback: T): Promise<T> {
  try {
    return await fn();
  } catch (err) {
    log.warn(`${label} failed (non-fatal): ${err instanceof Error ? err.message : err}`);
    return fallback;
  }
}
