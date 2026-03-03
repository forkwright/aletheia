// Typed event bus — fire-and-forget pub/sub for runtime lifecycle events
import { createLogger } from "./logger.js";

const log = createLogger("event-bus");

export type EventName =
  | "turn:before"
  | "turn:after"
  | "tool:called"
  | "tool:failed"
  | "status:update"
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
  | "config:reloaded"
  | "exec:denied"
  | "pipeline:error"
  | "history:orphan_repair"
  | "memory:health_degraded"
  | "memory:health_recovered"
  | "planning:project-created"
  | "planning:project-resumed"
  | "planning:phase-started"
  | "planning:phase-complete"
  | "planning:checkpoint"
  | "planning:complete"
  | "planning:requirement-changed"
  | "planning:phase-changed"
  | "planning:discussion-answered"
  | "planning:message-enqueued"
  | "planning:annotation-changed"
  | "planning:edit-recorded"
  | "planning:state-transition"
  | "planning:execution-progress"
  | "planning:execution-stuck"
  | "planning:iteration-capped"
  | "planning:verification-complete"
  | "task:created"
  | "task:updated"
  | "task:completed"
  | "task:deleted"
  | "task:bulk-created";

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
        // Catch async errors without blocking — emit() is synchronous, await not possible here
        if (result && typeof (result as Promise<void>).then === "function") {
          // eslint-disable-next-line promise/prefer-await-to-then -- sync emit() cannot await
          void (result as Promise<void>).then(
            undefined,
            (error: unknown) => {
              log.warn(`Event handler error [${event}]: ${error instanceof Error ? error.message : error}`);
            },
          );
        }
      } catch (error) {
        log.warn(`Event handler error [${event}]: ${error instanceof Error ? error.message : error}`);
      }
    }
  }

  listenerCount(event: EventName): number {
    return this.handlers.get(event)?.size ?? 0;
  }
}

export const eventBus = new EventBus();
