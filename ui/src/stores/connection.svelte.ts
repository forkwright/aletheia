import { initEventSource, closeEventSource, onGlobalEvent } from "../lib/events";

let status = $state<"connected" | "disconnected" | "connecting">("disconnected");

export function getConnectionStatus() {
  return status;
}

export function initConnection(): void {
  onGlobalEvent((event, data) => {
    if (event === "connection") {
      status = (data as { status: "connected" | "disconnected" }).status;
    }
  });
  initEventSource();
}

export function disconnect(): void {
  closeEventSource();
  status = "disconnected";
}
