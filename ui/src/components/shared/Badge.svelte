<script lang="ts">
  type Variant = "default" | "success" | "error" | "warning";
  let { text, variant = "default" }: { text: string; variant?: Variant } = $props();
</script>

<span class="badge {variant}">{text}</span>

<style>
  .badge {
    display: inline-block;
    padding: 2px 8px;
    border-radius: var(--radius-pill);
    font-size: var(--text-xs);
    font-weight: 600;
    white-space: nowrap;
  }
  .default {
    background: rgba(139, 148, 158, 0.15);
    color: var(--text-secondary);
  }
  .success {
    background: var(--status-success-bg-strong);
    color: var(--status-success);
  }
  .error {
    background: var(--status-error-bg-strong);
    color: var(--status-error);
  }
  .warning {
    background: var(--status-warning-bg-strong);
    color: var(--status-warning);
  }
</style>
