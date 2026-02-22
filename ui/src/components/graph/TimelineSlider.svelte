<script lang="ts">
  let {
    dateRange,
    onRangeChange,
    onClear,
  }: {
    dateRange: { oldest: string | null; newest: string | null } | null;
    onRangeChange: (since: string, until: string) => void;
    onClear: () => void;
  } = $props();

  // Default to last 90 days
  function defaultSince(): string {
    const d = new Date();
    d.setDate(d.getDate() - 90);
    return d.toISOString().slice(0, 10);
  }

  function defaultUntil(): string {
    return new Date().toISOString().slice(0, 10);
  }

  let sinceVal = $state(dateRange?.oldest?.slice(0, 10) || defaultSince());
  let untilVal = $state(defaultUntil());
  let active = $state(false);

  function apply() {
    active = true;
    onRangeChange(sinceVal + "T00:00:00Z", untilVal + "T23:59:59Z");
  }

  function clear() {
    active = false;
    onClear();
  }

  // Quick presets
  function preset(days: number) {
    const d = new Date();
    untilVal = d.toISOString().slice(0, 10);
    d.setDate(d.getDate() - days);
    sinceVal = d.toISOString().slice(0, 10);
    apply();
  }
</script>

<div class="timeline-slider" class:active>
  <div class="timeline-row">
    <span class="timeline-label">📅 Timeline</span>
    <div class="presets">
      <button class="preset-btn" onclick={() => preset(7)}>7d</button>
      <button class="preset-btn" onclick={() => preset(30)}>30d</button>
      <button class="preset-btn" onclick={() => preset(90)}>90d</button>
      <button class="preset-btn" onclick={() => preset(365)}>1y</button>
    </div>
    <div class="date-inputs">
      <input type="date" class="date-input" bind:value={sinceVal} />
      <span class="date-separator">→</span>
      <input type="date" class="date-input" bind:value={untilVal} />
    </div>
    <button class="apply-btn" onclick={apply}>Apply</button>
    {#if active}
      <button class="clear-btn" onclick={clear}>✕</button>
    {/if}
  </div>
</div>

<style>
  .timeline-slider {
    padding: 4px 12px;
    background: var(--bg-elevated, #181a1f);
    border-bottom: 1px solid var(--border, #30363d);
  }

  .timeline-slider.active {
    background: color-mix(in srgb, var(--accent, #9A7B4F) 8%, var(--bg-elevated, #181a1f));
    border-bottom-color: var(--accent, #9A7B4F);
  }

  .timeline-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .timeline-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary, #8b949e);
    white-space: nowrap;
  }

  .presets {
    display: flex;
    gap: 2px;
  }

  .preset-btn {
    background: var(--surface, #21262d);
    border: 1px solid var(--border, #30363d);
    color: var(--text-secondary, #8b949e);
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 10px;
    cursor: pointer;
  }
  .preset-btn:hover {
    color: var(--text, #e6edf3);
    border-color: var(--accent, #9A7B4F);
  }

  .date-inputs {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .date-input {
    background: var(--surface, #21262d);
    border: 1px solid var(--border, #30363d);
    color: var(--text, #e6edf3);
    padding: 2px 6px;
    border-radius: 4px;
    font-size: 11px;
    font-family: var(--font-mono, monospace);
    width: 120px;
  }
  .date-input:focus {
    outline: none;
    border-color: var(--accent, #9A7B4F);
  }

  .date-separator {
    color: var(--text-muted, #484f58);
    font-size: 11px;
  }

  .apply-btn {
    background: var(--accent, #9A7B4F);
    border: none;
    color: #0f1114;
    padding: 2px 10px;
    border-radius: 4px;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }
  .apply-btn:hover { opacity: 0.9; }

  .clear-btn {
    background: none;
    border: 1px solid var(--border, #30363d);
    color: var(--text-muted, #484f58);
    padding: 1px 5px;
    border-radius: 4px;
    font-size: 11px;
    cursor: pointer;
  }
  .clear-btn:hover {
    color: var(--red, #f85149);
    border-color: var(--red, #f85149);
  }

  @media (max-width: 768px) {
    .presets { display: none; }
    .date-input { width: 100px; }
  }
</style>
