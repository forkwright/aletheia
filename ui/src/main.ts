import { mount } from "svelte";
import App from "./App.svelte";
import "./styles/global.css";
import "./styles/hljs-dark.css";
import "./styles/chat-shared.css";

// Log main-thread blocks ≥50ms so freeze root cause shows in devtools console
if ("PerformanceObserver" in window) {
  try {
    const obs = new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        console.warn(`[perf] long task ${entry.duration.toFixed(0)}ms`, entry);
      }
    });
    obs.observe({ entryTypes: ["longtask"] });
  } catch { /* browser doesn't support longtask */ }
}

const app = mount(App, { target: document.getElementById("app")! });

export default app;
