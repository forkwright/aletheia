<script lang="ts">
  import Layout from "./components/layout/Layout.svelte";
  import { initConnection } from "./stores/connection.svelte";
  import { loadAgents } from "./stores/agents.svelte";
  import { getToken } from "./lib/api";

  $effect(() => {
    if (getToken()) {
      loadAgents();
      initConnection();
    }
  });

  function handleKeydown(e: KeyboardEvent) {
    // Cmd/Ctrl+N: New chat
    if ((e.metaKey || e.ctrlKey) && e.key === "n") {
      e.preventDefault();
      document.querySelector<HTMLButtonElement>(".new-chat-btn")?.click();
    }
    // "/" focuses the input when not already in an input
    if (e.key === "/" && !isInputFocused()) {
      e.preventDefault();
      document.querySelector<HTMLTextAreaElement>(".input-wrapper textarea")?.focus();
    }
    // Escape: blur input or abort streaming
    if (e.key === "Escape") {
      const abortBtn = document.querySelector<HTMLButtonElement>(".abort-btn");
      if (abortBtn) {
        abortBtn.click();
      } else {
        (document.activeElement as HTMLElement)?.blur();
      }
    }
  }

  function isInputFocused(): boolean {
    const el = document.activeElement;
    return el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement;
  }
</script>

<svelte:window onkeydown={handleKeydown} />
<Layout />
