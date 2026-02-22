<script lang="ts">
  import { onDestroy } from "svelte";

  let { startedAt = null }: { startedAt?: number | null } = $props();

  let elapsed = $state(0);
  let timer: ReturnType<typeof setInterval> | null = null;

  $effect(() => {
    if (timer) { clearInterval(timer); timer = null; }
    if (startedAt) {
      elapsed = Math.floor((Date.now() - startedAt) / 1000);
      timer = setInterval(() => {
        elapsed = Math.floor((Date.now() - startedAt!) / 1000);
      }, 1000);
    } else {
      elapsed = 0;
    }
  });

  onDestroy(() => { if (timer) clearInterval(timer); });
</script>

<span class="indicator">
  <span class="dots">
    <span class="dot"></span>
    <span class="dot"></span>
    <span class="dot"></span>
  </span>
  {#if startedAt && elapsed > 0}
    <span class="elapsed">Working... {elapsed}s</span>
  {/if}
</span>

<style>
  .indicator {
    display: inline-flex;
    gap: 8px;
    align-items: center;
    padding: 4px 0;
  }
  .dots {
    display: inline-flex;
    gap: 4px;
    align-items: center;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--text-muted);
    animation: bounce 1.4s ease infinite;
  }
  .dot:nth-child(2) { animation-delay: 0.2s; }
  .dot:nth-child(3) { animation-delay: 0.4s; }
  .elapsed {
    font-size: var(--text-xs);
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }

  @keyframes bounce {
    0%, 60%, 100% { transform: translateY(0); opacity: 0.4; }
    30% { transform: translateY(-4px); opacity: 1; }
  }
</style>
