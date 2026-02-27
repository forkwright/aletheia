import { SvelteSet } from "svelte/reactivity";
import { getEffectiveToken } from "./api";

type EventCallback = (event: string, data: unknown) => void;

let source: EventSource | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let heartbeatTimer: ReturnType<typeof setTimeout> | null = null;
let reconnectDelay = 1000;
const MAX_RECONNECT_DELAY = 30000;
const HEARTBEAT_TIMEOUT_MS = 45_000; // Server sends named "ping" events every ~15s
const listeners = new SvelteSet<EventCallback>();
let lastActiveTurns = $state<Record<string, number>>({});
let agentStatuses = $state<Record<string, string>>({});
let visibilityHandler: (() => void) | null = null;
let onlineHandler: (() => void) | null = null;

export function onGlobalEvent(cb: EventCallback): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

function dispatch(event: string, data: unknown) {
  for (const cb of listeners) {
    try { cb(event, data); } catch (e) { console.warn("[events] listener error:", e); }
  }
}

export function initEventSource(): void {
  if (source) return;
  connect();
  installLifecycleHandlers();
}

export function closeEventSource(): void {
  if (reconnectTimer) { clearTimeout(reconnectTimer); reconnectTimer = null; }
  if (heartbeatTimer) { clearTimeout(heartbeatTimer); heartbeatTimer = null; }
  if (source) {
    source.close();
    source = null;
  }
  removeLifecycleHandlers();
}

/**
 * Immediately reconnect SSE — used when the page regains visibility or
 * comes back online. Clears any pending exponential-backoff timer and
 * connects with a fresh 1s delay floor.
 */
function forceReconnect() {
  if (source && source.readyState === EventSource.OPEN) return; // already healthy
  if (reconnectTimer) { clearTimeout(reconnectTimer); reconnectTimer = null; }
  if (heartbeatTimer) { clearTimeout(heartbeatTimer); heartbeatTimer = null; }
  if (source) { source.close(); source = null; }
  reconnectDelay = 1000; // reset backoff
  connect();
}

/**
 * Mobile browsers freeze/kill SSE connections when:
 *  - Tab goes to background (visibilitychange)
 *  - Phone sleeps (same event)
 *  - Network changes (online/offline events)
 *
 * Without explicit reconnect on resume, users see stale UI until the
 * 45s heartbeat timeout fires — which feels broken.
 */
function installLifecycleHandlers() {
  if (visibilityHandler) return; // already installed

  visibilityHandler = () => {
    if (document.visibilityState === "visible") {
      // Tab is back — reconnect immediately
      forceReconnect();
    }
  };
  document.addEventListener("visibilitychange", visibilityHandler);

  onlineHandler = () => {
    // Network came back — reconnect immediately
    forceReconnect();
  };
  window.addEventListener("online", onlineHandler);
}

function removeLifecycleHandlers() {
  if (visibilityHandler) {
    document.removeEventListener("visibilitychange", visibilityHandler);
    visibilityHandler = null;
  }
  if (onlineHandler) {
    window.removeEventListener("online", onlineHandler);
    onlineHandler = null;
  }
}

function resetHeartbeat() {
  if (heartbeatTimer) clearTimeout(heartbeatTimer);
  heartbeatTimer = setTimeout(() => {
    // No activity for 45s — connection is likely dead
    if (source) {
      source.close();
      source = null;
      dispatch("connection", { status: "disconnected" });
      scheduleReconnect();
    }
  }, HEARTBEAT_TIMEOUT_MS);
}

function connect() {
  // Clean up any existing connection
  if (source) {
    source.close();
    source = null;
  }

  const token = getEffectiveToken();
  const base = import.meta.env.DEV ? "" : window.location.origin;
  const url = `${base}/api/events${token ? `?token=${encodeURIComponent(token)}` : ""}`;

  source = new EventSource(url);

  source.onopen = () => {
    reconnectDelay = 1000;
    resetHeartbeat();
    dispatch("connection", { status: "connected" });
  };

  source.onerror = () => {
    dispatch("connection", { status: "disconnected" });
    if (heartbeatTimer) { clearTimeout(heartbeatTimer); heartbeatTimer = null; }
    source?.close();
    source = null;
    scheduleReconnect();
  };

  source.addEventListener("init", (e) => {
    resetHeartbeat();
    try {
      const data = JSON.parse((e as MessageEvent).data);
      const newActiveTurns: Record<string, number> = data.activeTurns ?? {};
      for (const agentId of Object.keys(lastActiveTurns)) {
        if (!(agentId in newActiveTurns)) {
          newActiveTurns[agentId] = 0;
        }
      }
      // Only reassign if values actually changed (prevents $state object thrashing)
      if (JSON.stringify(lastActiveTurns) !== JSON.stringify(newActiveTurns)) {
        lastActiveTurns = newActiveTurns;
      }
      dispatch("init", { ...data, activeTurns: lastActiveTurns });
    } catch (e) { console.warn("[events] SSE init parse error:", e); }
  });

  // Forward server event types (only types the server SSE route actually emits)
  const eventTypes = [
    "turn:before", "turn:after",
    "tool:called", "tool:failed", "status:update",
    "session:created", "session:archived",
    "distill:before", "distill:stage", "distill:after",
    "planning:project-created", "planning:project-resumed",
    "planning:phase-started", "planning:phase-complete",
    "planning:checkpoint", "planning:complete",
    "planning:requirement-changed", "planning:phase-changed",
    "planning:discussion-answered",
    "task:created", "task:updated", "task:completed",
    "task:deleted", "task:bulk-created",
  ];
  for (const type of eventTypes) {
    source.addEventListener(type, (e) => {
      resetHeartbeat();
      try {
        const data = JSON.parse((e as MessageEvent).data);
        if (type === "turn:before" && data.nousId) {
          lastActiveTurns = { ...lastActiveTurns, [data.nousId]: (lastActiveTurns[data.nousId] ?? 0) + 1 };
        } else if (type === "turn:after" && data.nousId) {
          lastActiveTurns = { ...lastActiveTurns, [data.nousId]: Math.max(0, (lastActiveTurns[data.nousId] ?? 1) - 1) };
          // Clear status when turn ends
          if ((lastActiveTurns[data.nousId] ?? 0) <= 0) {
            agentStatuses = { ...agentStatuses, [data.nousId]: "" };
          }
        } else if (type === "status:update" && data.nousId && data.status) {
          agentStatuses = { ...agentStatuses, [data.nousId]: data.status };
        }
        dispatch(type, data);
      } catch (e) { console.warn("[events] SSE event parse error:", e, "type:", type); }
    });
  }

  // Server sends named "ping" events (not SSE comments) so they're actually
  // delivered to listeners. SSE comments (": ping") are silently consumed by
  // the browser's EventSource parser and never fire any handler.
  source.addEventListener("ping", () => { resetHeartbeat(); });
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    reconnectDelay = Math.min(reconnectDelay * 2, MAX_RECONNECT_DELAY);
    connect();
  }, reconnectDelay);
}

export function getActiveTurns(): Record<string, number> {
  return lastActiveTurns;
}

export function getAgentStatus(agentId: string): string {
  return agentStatuses[agentId] ?? "";
}

export function getConnectionStatus(): "connected" | "disconnected" | "connecting" {
  if (!source) return "disconnected";
  if (source.readyState === EventSource.CONNECTING) return "connecting";
  if (source.readyState === EventSource.OPEN) return "connected";
  return "disconnected";
}
