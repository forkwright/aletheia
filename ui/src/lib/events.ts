import { getEffectiveToken } from "./api";

type EventCallback = (event: string, data: unknown) => void;

let source: EventSource | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let reconnectDelay = 1000;
const MAX_RECONNECT_DELAY = 30000;
const listeners = new Set<EventCallback>();

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
  if (reconnectTimer) clearTimeout(reconnectTimer);
  if (source) {
    source.close();
    source = null;
  }
}

function connect() {
  const token = getEffectiveToken();
  const base = import.meta.env.DEV ? "" : window.location.origin;
  const url = `${base}/api/events${token ? `?token=${encodeURIComponent(token)}` : ""}`;

  source = new EventSource(url);

  source.onopen = () => {
    reconnectDelay = 1000;
    dispatch("connection", { status: "connected" });
  };

  source.onerror = () => {
    dispatch("connection", { status: "disconnected" });
    source?.close();
    source = null;
    scheduleReconnect();
  };

  source.addEventListener("init", (e) => {
    try {
      dispatch("init", JSON.parse((e as MessageEvent).data));
    } catch { /* ignore */ }
  });

  // Forward all other event types
  const eventTypes = [
    "turn:before", "turn:after", "turn:text_delta", "turn:tool_start", "turn:tool_result",
    "tool:called", "tool:failed", "session:created", "session:archived",
    "distill:before", "distill:stage", "distill:after",
  ];
  for (const type of eventTypes) {
    source.addEventListener(type, (e) => {
      try {
        dispatch(type, JSON.parse((e as MessageEvent).data));
      } catch { /* ignore */ }
    });
  }
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    reconnectDelay = Math.min(reconnectDelay * 2, MAX_RECONNECT_DELAY);
    connect();
  }, reconnectDelay);
}

export function getConnectionStatus(): "connected" | "disconnected" | "connecting" {
  if (!source) return "disconnected";
  if (source.readyState === EventSource.CONNECTING) return "connecting";
  if (source.readyState === EventSource.OPEN) return "connected";
  return "disconnected";
}
