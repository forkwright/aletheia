// Typed event bus — fire-and-forget pub/sub for runtime lifecycle events
import { createLogger } from "./logger.js";

const log = createLogger("event-bus");

export type EventName =
  | "turn:before"
  | "turn:after"
  | "tool:called"
  | "tool:failed"
  | "distill:before"
  | "distill:stage"
  | "distill:after"
  | "session:created"
  | "session:archived"
  | "memory:added"
  | "memory:retracted"
  | "signal:received"
  | "boot:start"
  | "boot:ready"
  | "pipeline:error"
  | "history:orphan_repair";

export type EventPayload = Record<string, unknown>;
export type EventHandler = (payload: EventPayload) => void | Promise<void>;

class EventBus {
  private handlers = new Map<EventName, Set<EventHandler>>();

  on(event: EventName, handler: EventHandler): void {
    let set = this.handlers.get(event);
    if (!set) {
      set = new Set();
      this.handlers.set(event, set);
    }
    set.add(handler);
  }

  off(event: EventName, handler: EventHandler): void {
    const set = this.handlers.get(event);
    if (set) {
      set.delete(handler);
      if (set.size === 0) this.handlers.delete(event);
    }
  }

  emit(event: EventName, payload: EventPayload): void {
    const set = this.handlers.get(event);
    if (!set || set.size === 0) return;

    for (const handler of set) {
      try {
        const result = handler(payload);
        // Catch async errors without blocking
        if (result && typeof (result as Promise<void>).catch === "function") {
          (result as Promise<void>).catch((err) => {
            log.warn(`Event handler error [${event}]: ${err instanceof Error ? err.message : err}`);
          });
        }
      } catch (err) {
        log.warn(`Event handler error [${event}]: ${err instanceof Error ? err.message : err}`);
      }
    }
  }

  listenerCount(event: EventName): number {
    return this.handlers.get(event)?.size ?? 0;
  }
}

export const eventBus = new EventBus();
