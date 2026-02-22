import { getEffectiveToken } from "./api";

type EventCallback = (event: string, data: unknown) => void;

let source: EventSource | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let heartbeatTimer: ReturnType<typeof setTimeout> | null = null;
let reconnectDelay = 1000;
const MAX_RECONNECT_DELAY = 30000;
const HEARTBEAT_TIMEOUT_MS = 45_000; // Server sends pings every ~30s
const listeners = new Set<EventCallback>();
let lastActiveTurns = $state<Record<string, number>>({});
let agentStatuses = $state<Record<string, string>>({});

export function onGlobalEvent(cb: EventCallback): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

function dispatch(event: string, data: unknown) {
  for (const cb of listeners) {
    try { cb(event, data); } catch { /* ignore */ }
  }
}

export function initEventSource(): void {
  if (source) return;
  connect();
}

export function closeEventSource(): void {
  if (reconnectTimer) { clearTimeout(reconnectTimer); reconnectTimer = null; }
  if (heartbeatTimer) { clearTimeout(heartbeatTimer); heartbeatTimer = null; }
  if (source) {
    source.close();
    source = null;
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
    } catch { /* ignore */ }
  });

  // Forward server event types (only types the server SSE route actually emits)
  const eventTypes = [
    "turn:before", "turn:after",
    "tool:called", "tool:failed", "status:update",
    "session:created", "session:archived",
    "distill:before", "distill:stage", "distill:after",
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
      } catch { /* ignore */ }
    });
  }

  // SSE comment lines (:ping) don't fire event listeners, but onmessage catches them
  source.onmessage = () => { resetHeartbeat(); };
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
