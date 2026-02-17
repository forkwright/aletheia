<script lang="ts">
  let {
    isStreaming,
    onSend,
    onAbort,
    contextPercent = 0,
    slashCommands = [],
  }: {
    isStreaming: boolean;
    onSend: (text: string) => void;
    onAbort: () => void;
    contextPercent?: number;
    slashCommands?: Array<{ command: string; description: string }>;
  } = $props();

  let text = $state("");
  let textarea = $state<HTMLTextAreaElement | null>(null);
  let showSlashMenu = $state(false);
  let selectedSlashIdx = $state(0);

  let filteredCommands = $derived(
    text.startsWith("/")
      ? slashCommands.filter((c) => c.command.startsWith(text.trim()))
      : [],
  );

  $effect(() => {
    showSlashMenu = filteredCommands.length > 0 && text.startsWith("/");
    if (showSlashMenu) selectedSlashIdx = 0;
  });

  function handleKeydown(e: KeyboardEvent) {
    if (showSlashMenu) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        selectedSlashIdx = (selectedSlashIdx + 1) % filteredCommands.length;
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        selectedSlashIdx = (selectedSlashIdx - 1 + filteredCommands.length) % filteredCommands.length;
        return;
      }
      if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey)) {
        e.preventDefault();
        const cmd = filteredCommands[selectedSlashIdx];
        if (cmd) {
          text = cmd.command;
          showSlashMenu = false;
          if (e.key === "Enter") submit();
        }
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        showSlashMenu = false;
        return;
      }
    }

    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  }

  function submit() {
    const trimmed = text.trim();
    if (!trimmed || isStreaming) return;
    onSend(trimmed);
    text = "";
    showSlashMenu = false;
    if (textarea) textarea.style.height = "40px";
  }

  function autoResize() {
    if (!textarea) return;
    textarea.style.height = "40px";
    textarea.style.height = Math.min(textarea.scrollHeight, 200) + "px";
  }

  function selectSlashCommand(idx: number) {
    const cmd = filteredCommands[idx];
    if (cmd) {
      text = cmd.command;
      showSlashMenu = false;
      submit();
    }
  }

  $effect(() => {
    // Focus textarea when not streaming
    if (!isStreaming && textarea) {
      textarea.focus();
    }
  });

  // Context bar color based on proximity to distillation
  let contextColor = $derived(
    contextPercent > 80 ? "var(--red)" :
    contextPercent > 60 ? "var(--yellow)" :
    "var(--accent)"
  );
</script>

<div class="input-bar">
  {#if contextPercent > 0}
    <div class="context-bar" title="Context window: {contextPercent}% used">
      <div
        class="context-fill"
        style="width: {contextPercent}%; background: {contextColor};"
      ></div>
    </div>
  {/if}
  {#if isStreaming}
    <button class="abort-btn" onclick={onAbort}>
      Stop generating
    </button>
  {:else}
    <div class="input-area">
      {#if showSlashMenu}
        <div class="slash-menu">
          {#each filteredCommands as cmd, i}
            <button
              class="slash-item"
              class:selected={i === selectedSlashIdx}
              onclick={() => selectSlashCommand(i)}
            >
              <span class="slash-cmd">{cmd.command}</span>
              <span class="slash-desc">{cmd.description}</span>
            </button>
          {/each}
        </div>
      {/if}
      <div class="input-wrapper">
        <textarea
          bind:this={textarea}
          bind:value={text}
          onkeydown={handleKeydown}
          oninput={autoResize}
          placeholder="Type a message... (/ for commands)"
          rows="1"
          disabled={isStreaming}
        ></textarea>
        <button
          class="send-btn"
          onclick={submit}
          disabled={!text.trim() || isStreaming}
        >
          Send
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .input-bar {
    border-top: 1px solid var(--border);
    background: var(--bg-elevated);
    flex-shrink: 0;
  }
  .context-bar {
    height: 2px;
    background: var(--surface);
    overflow: hidden;
  }
  .context-fill {
    height: 100%;
    transition: width 0.5s ease, background 0.5s ease;
  }
  .input-area {
    position: relative;
    padding: 12px 16px;
  }
  .input-wrapper {
    display: flex;
    gap: 8px;
    align-items: flex-end;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 4px 4px 4px 12px;
    transition: border-color 0.15s;
  }
  .input-wrapper:focus-within {
    border-color: var(--accent);
  }
  textarea {
    flex: 1;
    background: transparent;
    border: none;
    color: var(--text);
    font-family: var(--font-sans);
    font-size: 14px;
    line-height: 1.5;
    resize: none;
    min-height: 40px;
    max-height: 200px;
    padding: 8px 0;
    outline: none;
  }
  textarea::placeholder {
    color: var(--text-muted);
  }
  .send-btn {
    background: var(--accent);
    border: none;
    color: #fff;
    padding: 8px 16px;
    border-radius: var(--radius-sm);
    font-size: 13px;
    font-weight: 500;
    flex-shrink: 0;
    transition: background 0.15s, opacity 0.15s;
  }
  .send-btn:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .send-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .abort-btn {
    width: calc(100% - 32px);
    margin: 12px 16px;
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid rgba(248, 81, 73, 0.3);
    color: var(--red);
    padding: 10px;
    border-radius: var(--radius);
    font-size: 13px;
    font-weight: 500;
    transition: background 0.15s;
  }
  .abort-btn:hover {
    background: rgba(248, 81, 73, 0.2);
  }
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
    box-shadow: 0 -4px 16px rgba(0, 0, 0, 0.3);
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
    font-size: 13px;
    text-align: left;
    transition: background 0.1s;
  }
  .slash-item:hover,
  .slash-item.selected {
    background: var(--surface-hover);
  }
  .slash-cmd {
    font-family: var(--font-mono);
    color: var(--accent);
    font-weight: 600;
    font-size: 13px;
    min-width: 60px;
  }
  .slash-desc {
    color: var(--text-secondary);
    font-size: 12px;
  }
</style>
