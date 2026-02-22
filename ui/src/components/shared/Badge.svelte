<script lang="ts">
  type Variant = "default" | "success" | "error" | "warning";
  let { text, variant = "default" }: { text: string; variant?: Variant } = $props();
</script>

<span class="badge {variant}">{text}</span>

<style>
  .badge {
    display: inline-block;
    padding: 2px 8px;
    border-radius: 12px;
    font-size: var(--text-xs);
    font-weight: 600;
    white-space: nowrap;
  }
  .default {
    background: rgba(139, 148, 158, 0.15);
    color: var(--text-secondary);
  }
  .success {
    background: rgba(63, 185, 80, 0.15);
    color: var(--status-success);
  }
  .error {
    background: rgba(248, 81, 73, 0.15);
    color: var(--status-error);
  }
  .warning {
    background: rgba(210, 153, 34, 0.15);
    color: var(--status-warning);
  }
</style>
