<script lang="ts">
  let {
    isStreaming,
    onSend,
    onAbort,
  }: {
    isStreaming: boolean;
    onSend: (text: string) => void;
    onAbort: () => void;
  } = $props();

  let text = $state("");
  let textarea = $state<HTMLTextAreaElement | null>(null);

  function handleKeydown(e: KeyboardEvent) {
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
    if (textarea) textarea.style.height = "40px";
  }

  function autoResize() {
    if (!textarea) return;
    textarea.style.height = "40px";
    textarea.style.height = Math.min(textarea.scrollHeight, 200) + "px";
  }

  $effect(() => {
    // Focus textarea when not streaming
    if (!isStreaming && textarea) {
      textarea.focus();
    }
  });
</script>

<div class="input-bar">
  {#if isStreaming}
    <button class="abort-btn" onclick={onAbort}>
      Stop generating
    </button>
  {:else}
    <div class="input-wrapper">
      <textarea
        bind:this={textarea}
        bind:value={text}
        onkeydown={handleKeydown}
        oninput={autoResize}
        placeholder="Type a message..."
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
  {/if}
</div>

<style>
  .input-bar {
    padding: 12px 16px;
    border-top: 1px solid var(--border);
    background: var(--bg-elevated);
    flex-shrink: 0;
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
    width: 100%;
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
</style>
