// In-memory delivery queue for undelivered turn responses
// When a client disconnects during a long turn, the response would be lost.
// This queue captures completed-but-undelivered responses and flushes them
// when the client reconnects.

import { createLogger } from "../koina/logger.js";

const log = createLogger("pylon.delivery");

export interface QueuedDelivery {
  sessionId: string;
  nousId: string;
  turnId: string;
  /** The complete turn_complete event payload */
  payload: Record<string, unknown>;
  attempts: number;
  lastAttemptAt: number;
  createdAt: number;
}

/** Maximum entries per session to prevent unbounded growth */
const MAX_PER_SESSION = 10;

/** Maximum age before entries are discarded (1 hour) */
const MAX_AGE_MS = 60 * 60 * 1000;

/** Cleanup interval (5 minutes) */
const CLEANUP_INTERVAL_MS = 5 * 60 * 1000;

export class DeliveryQueue {
  private queue = new Map<string, QueuedDelivery[]>(); // sessionId → entries
  private cleanupTimer: ReturnType<typeof setInterval> | null = null;

  constructor() {
    this.cleanupTimer = setInterval(() => this.cleanup(), CLEANUP_INTERVAL_MS);
    // Prevent the timer from keeping the process alive
    if (this.cleanupTimer.unref) this.cleanupTimer.unref();
  }

  /**
   * Enqueue a turn response that couldn't be delivered to the client.
   */
  enqueue(entry: Omit<QueuedDelivery, "attempts" | "lastAttemptAt" | "createdAt">): void {
    const now = Date.now();
    const delivery: QueuedDelivery = {
      ...entry,
      attempts: 1,
      lastAttemptAt: now,
      createdAt: now,
    };

    const key = entry.sessionId;
    const existing = this.queue.get(key) ?? [];

    // Cap per-session entries — drop oldest if full
    if (existing.length >= MAX_PER_SESSION) {
      const dropped = existing.shift();
      log.warn(`Delivery queue full for session ${key} — dropped oldest (turn ${dropped?.turnId})`);
    }

    existing.push(delivery);
    this.queue.set(key, existing);
    log.info(`Queued undelivered response for session ${key} (turn ${entry.turnId})`);
  }

  /**
   * Flush all pending deliveries for a session.
   * Called when a client reconnects to a session.
   * Returns the entries and removes them from the queue.
   */
  flush(sessionId: string): QueuedDelivery[] {
    const entries = this.queue.get(sessionId);
    if (!entries || entries.length === 0) return [];

    this.queue.delete(sessionId);
    log.info(`Flushed ${entries.length} pending delivery(ies) for session ${sessionId}`);
    return entries;
  }

  /**
   * Flush all pending deliveries for a specific nous (agent).
   * Used when we know the agent but not the exact session.
   */
  flushByNous(nousId: string): QueuedDelivery[] {
    const result: QueuedDelivery[] = [];
    for (const [sessionId, entries] of this.queue) {
      const matching = entries.filter(e => e.nousId === nousId);
      const remaining = entries.filter(e => e.nousId !== nousId);

      if (matching.length > 0) {
        result.push(...matching);
        if (remaining.length > 0) {
          this.queue.set(sessionId, remaining);
        } else {
          this.queue.delete(sessionId);
        }
      }
    }
    if (result.length > 0) {
      log.info(`Flushed ${result.length} pending delivery(ies) for nous ${nousId}`);
    }
    return result;
  }

  /**
   * Check if there are pending deliveries for a session.
   */
  hasPending(sessionId: string): boolean {
    const entries = this.queue.get(sessionId);
    return !!entries && entries.length > 0;
  }

  /**
   * Get count of all pending deliveries across all sessions.
   */
  get size(): number {
    let total = 0;
    for (const entries of this.queue.values()) {
      total += entries.length;
    }
    return total;
  }

  /**
   * Remove expired entries.
   */
  private cleanup(): void {
    const now = Date.now();
    let removed = 0;

    for (const [sessionId, entries] of this.queue) {
      const fresh = entries.filter(e => now - e.createdAt < MAX_AGE_MS);
      removed += entries.length - fresh.length;

      if (fresh.length === 0) {
        this.queue.delete(sessionId);
      } else if (fresh.length < entries.length) {
        this.queue.set(sessionId, fresh);
      }
    }

    if (removed > 0) {
      log.debug(`Delivery queue cleanup: removed ${removed} expired entries`);
    }
  }

  /**
   * Stop the cleanup timer (for graceful shutdown).
   */
  dispose(): void {
    if (this.cleanupTimer) {
      clearInterval(this.cleanupTimer);
      this.cleanupTimer = null;
    }
  }
}
