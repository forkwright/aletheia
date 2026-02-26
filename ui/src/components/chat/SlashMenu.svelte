<script lang="ts">
  type SlashCommand = { command: string; description: string };

  let { commands, selectedIndex, onSelect }: {
    commands: SlashCommand[];
    selectedIndex: number;
    onSelect: (cmd: SlashCommand) => void;
  } = $props();
</script>

<div class="slash-menu">
  {#each commands as cmd, i (cmd.command)}
    <button
      class="slash-item"
      class:selected={i === selectedIndex}
      onclick={() => onSelect(cmd)}
    >
      <span class="slash-cmd">{cmd.command}</span>
      <span class="slash-desc">{cmd.description}</span>
    </button>
  {/each}
</div>

<style>
  .slash-menu {
    position: absolute;
    bottom: 100%;
    left: 16px;
    right: 16px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    margin-bottom: 4px;
    overflow: hidden;
    z-index: 20;
    box-shadow: 0 -4px 16px var(--shadow-md);
  }
  .slash-item {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 8px 12px;
    background: transparent;
    border: none;
    color: var(--text);
    font-size: var(--text-sm);
    text-align: left;
    transition: background var(--transition-quick);
  }
  .slash-item:hover,
  .slash-item.selected {
    background: var(--surface-hover);
  }
  .slash-cmd {
    font-family: var(--font-mono);
    color: var(--accent);
    font-weight: 600;
    font-size: var(--text-sm);
    min-width: 60px;
  }
  .slash-desc {
    color: var(--text-secondary);
    font-size: var(--text-sm);
  }

  @media (max-width: 768px) {
    .slash-menu {
      left: 10px;
      right: 10px;
      max-height: 40vh;
      overflow-y: auto;
      -webkit-overflow-scrolling: touch;
    }
    .slash-item {
      padding: 12px 14px;
      min-height: 44px;
    }
  }
</style>
