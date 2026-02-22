import { initEventSource, closeEventSource, onGlobalEvent } from "../lib/events";

let status = $state<"connected" | "disconnected" | "connecting">("disconnected");
let unsub: (() => void) | null = null;

export function getConnectionStatus() {
  return status;
}

export function initConnection(): void {
  unsub?.();
  unsub = onGlobalEvent((event, data) => {
    if (event === "connection") {
      status = (data as { status: "connected" | "disconnected" }).status;
    }
  });
  initEventSource();
}

export function disconnect(): void {
  unsub?.();
  unsub = null;
  closeEventSource();
  status = "disconnected";
}
