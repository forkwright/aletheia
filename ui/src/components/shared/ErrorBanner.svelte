<script lang="ts">
  let { message, onDismiss }: { message: string; onDismiss?: () => void } = $props();
</script>

<div class="error-banner">
  <span class="error-text">{message}</span>
  {#if onDismiss}
    <button class="dismiss" onclick={onDismiss}>x</button>
  {/if}
</div>

<style>
  .error-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid rgba(248, 81, 73, 0.3);
    border-radius: var(--radius-sm);
    color: var(--red);
    font-size: 13px;
  }
  .error-text {
    flex: 1;
  }
  .dismiss {
    background: none;
    border: none;
    color: var(--red);
    font-size: 14px;
    padding: 0 4px;
    opacity: 0.7;
  }
  .dismiss:hover {
    opacity: 1;
  }
</style>
