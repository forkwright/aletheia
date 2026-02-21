<script lang="ts">
  import type { MediaItem } from "../../lib/types";

  let {
    isStreaming,
    onSend,
    onAbort,
    contextPercent = 0,
    slashCommands = [],
  }: {
    isStreaming: boolean;
    onSend: (text: string, media?: MediaItem[]) => void;
    onAbort: () => void;
    contextPercent?: number;
    slashCommands?: Array<{ command: string; description: string }>;
  } = $props();

  let text = $state("");
  let queued = $state<string | null>(null);
  let textarea = $state<HTMLTextAreaElement | null>(null);
  let showSlashMenu = $state(false);
  let selectedSlashIdx = $state(0);
  let attachments = $state<MediaItem[]>([]);
  let fileInput = $state<HTMLInputElement | null>(null);
  let isDragOver = $state(false);

  const MAX_FILE_SIZE = 25 * 1024 * 1024; // 25MB
  const IMAGE_TYPES = ["image/jpeg", "image/png", "image/gif", "image/webp"];
  const DOC_TYPES = ["application/pdf"];
  const TEXT_TYPES = [
    "text/plain", "text/csv", "text/markdown", "text/html", "text/xml",
    "application/json", "application/xml",
  ];
  const ACCEPTED_TYPES = [...IMAGE_TYPES, ...DOC_TYPES, ...TEXT_TYPES];

  function isTextLikeType(type: string): boolean {
    return TEXT_TYPES.includes(type) || type.startsWith("text/");
  }

  function isImageType(type: string): boolean {
    return IMAGE_TYPES.includes(type);
  }

  let filteredCommands = $derived(
    text.startsWith("/")
      ? slashCommands.filter((c) => c.command.startsWith(text.trim()))
      : [],
  );

  $effect(() => {
    showSlashMenu = filteredCommands.length > 0 && text.startsWith("/");
    if (showSlashMenu) selectedSlashIdx = 0;
  });

  // When streaming ends, send queued message
  $effect(() => {
    if (!isStreaming && queued) {
      const msg = queued;
      queued = null;
      onSend(msg);
    }
  });

  function inferContentType(file: File): string | null {
    if (file.type && (ACCEPTED_TYPES.includes(file.type) || file.type.startsWith("text/"))) {
      return file.type;
    }
    // Infer from extension for common types
    const ext = file.name.split(".").pop()?.toLowerCase();
    const extMap: Record<string, string> = {
      pdf: "application/pdf",
      json: "application/json",
      xml: "application/xml",
      csv: "text/csv",
      md: "text/markdown",
      txt: "text/plain",
      html: "text/html",
      htm: "text/html",
      yaml: "text/yaml",
      yml: "text/yaml",
      toml: "text/plain",
      log: "text/plain",
      py: "text/plain",
      js: "text/plain",
      ts: "text/plain",
      sh: "text/plain",
      sql: "text/plain",
      css: "text/plain",
      rs: "text/plain",
      go: "text/plain",
      java: "text/plain",
      c: "text/plain",
      cpp: "text/plain",
      h: "text/plain",
      rb: "text/plain",
      swift: "text/plain",
    };
    if (ext && extMap[ext]) return extMap[ext];
    return null;
  }

  function fileToMediaItem(file: File): Promise<MediaItem | null> {
    return new Promise((resolve) => {
      const contentType = inferContentType(file);
      if (!contentType) {
        resolve(null);
        return;
      }
      if (file.size > MAX_FILE_SIZE) {
        resolve(null);
        return;
      }
      const reader = new FileReader();
      reader.onload = () => {
        const result = reader.result as string;
        // Strip data URI prefix — backend expects raw base64
        const base64Match = result.match(/^data:[^;]+;base64,(.+)$/);
        const data = base64Match ? base64Match[1] : result;
        resolve({
          contentType,
          data,
          filename: file.name,
        });
      };
      reader.onerror = () => resolve(null);
      reader.readAsDataURL(file);
    });
  }

  async function handleFiles(files: FileList | File[]) {
    const fileArray = Array.from(files);
    for (const file of fileArray) {
      if (attachments.length >= 4) break; // max 4 attachments
      const item = await fileToMediaItem(file);
      if (item) {
        attachments = [...attachments, item];
      }
    }
  }

  function removeAttachment(idx: number) {
    attachments = attachments.filter((_, i) => i !== idx);
  }

  function handleAttachClick() {
    fileInput?.click();
  }

  function handleFileSelect(e: Event) {
    const input = e.target as HTMLInputElement;
    if (input.files) {
      handleFiles(input.files);
      input.value = ""; // Reset so same file can be re-selected
    }
  }

  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    isDragOver = true;
  }

  function handleDragLeave(e: DragEvent) {
    e.preventDefault();
    isDragOver = false;
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    isDragOver = false;
    if (e.dataTransfer?.files) {
      handleFiles(e.dataTransfer.files);
    }
  }

  function handlePaste(e: ClipboardEvent) {
    const items = e.clipboardData?.items;
    if (!items) return;
    const pastedFiles: File[] = [];
    for (const item of items) {
      if (item.kind === "file") {
        const file = item.getAsFile();
        if (file && inferContentType(file)) pastedFiles.push(file);
      }
    }
    if (pastedFiles.length > 0) {
      e.preventDefault();
      handleFiles(pastedFiles);
    }
  }

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
    const hasContent = trimmed || attachments.length > 0;
    if (!hasContent) return;

    if (isStreaming) {
      if (trimmed) {
        queued = trimmed;
        text = "";
        if (textarea) textarea.style.height = "40px";
      }
      return;
    }

    const media = attachments.length > 0 ? [...attachments] : undefined;
    const hasImages = media?.some(m => isImageType(m.contentType));
    const defaultPrompt = hasImages ? "What's in this image?" : "Please review this file.";
    const message = trimmed || (media ? defaultPrompt : "");

    onSend(message, media);
    text = "";
    attachments = [];
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
    if (!isStreaming && textarea) {
      textarea.focus();
    }
  });

  let contextColor = $derived(
    contextPercent > 80 ? "var(--red)" :
    contextPercent > 60 ? "var(--yellow)" :
    "var(--accent)"
  );

  let hasContent = $derived(text.trim().length > 0 || attachments.length > 0);
</script>

<div
  class="input-bar"
  class:drag-over={isDragOver}
  ondragover={handleDragOver}
  ondragleave={handleDragLeave}
  ondrop={handleDrop}
>
  {#if contextPercent > 0}
    <div class="context-bar" title="Context window: {contextPercent}% used">
      <div
        class="context-fill"
        style="width: {contextPercent}%; background: {contextColor};"
      ></div>
    </div>
  {/if}
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
    {#if queued}
      <div class="queued-indicator">
        <span class="queued-text">Queued: {queued.length > 60 ? queued.slice(0, 60) + "…" : queued}</span>
        <button class="queued-cancel" onclick={() => { queued = null; }} aria-label="Cancel queued message">×</button>
      </div>
    {/if}
    {#if attachments.length > 0}
      <div class="attachment-preview">
        {#each attachments as att, i}
          <div class="attachment-thumb">
            {#if isImageType(att.contentType)}
              <img src="data:{att.contentType};base64,{att.data}" alt={att.filename ?? "attachment"} />
            {:else}
              <div class="file-icon">
                {#if att.contentType === "application/pdf"}
                  <span class="file-emoji">📄</span>
                {:else if isTextLikeType(att.contentType)}
                  <span class="file-emoji">📝</span>
                {:else}
                  <span class="file-emoji">📎</span>
                {/if}
              </div>
            {/if}
            <button class="remove-btn" onclick={() => removeAttachment(i)} aria-label="Remove attachment">×</button>
            {#if att.filename}
              <span class="attachment-name">{att.filename}</span>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
    <div class="input-wrapper" class:streaming={isStreaming}>
      {#if isStreaming}
        <button class="stop-btn" onclick={onAbort} aria-label="Stop generating" title="Stop generating (Esc)">
          <span class="stop-icon">■</span>
        </button>
      {/if}
      <button
        class="attach-btn"
        onclick={handleAttachClick}
        title="Attach file (images, PDFs, text, code)"
        aria-label="Attach file"
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M21.44 11.05l-9.19 9.19a6 6 0 01-8.49-8.49l9.19-9.19a4 4 0 015.66 5.66l-9.2 9.19a2 2 0 01-2.83-2.83l8.49-8.48" />
        </svg>
      </button>
      <textarea
        bind:this={textarea}
        bind:value={text}
        onkeydown={handleKeydown}
        oninput={autoResize}
        onpaste={handlePaste}
        placeholder={isStreaming ? "Type to queue a message..." : "Type a message... (/ for commands)"}
        rows="1"
      ></textarea>
      <button
        class="send-btn"
        onclick={submit}
        disabled={!hasContent}
        class:queuing={isStreaming && text.trim().length > 0}
      >
        {isStreaming && text.trim().length > 0 ? "Queue" : "Send"}
      </button>
    </div>
    <input
      bind:this={fileInput}
      type="file"
      accept="image/*,application/pdf,.json,.xml,.csv,.md,.txt,.html,.yaml,.yml,.toml,.log,.py,.js,.ts,.sh,.sql,.css,.rs,.go,.java,.c,.cpp,.h,.rb,.swift"
      multiple
      onchange={handleFileSelect}
      class="hidden-file-input"
    />
  </div>
  {#if isDragOver}
    <div class="drag-overlay">
      <div class="drag-label">Drop file to attach</div>
    </div>
  {/if}
</div>

<style>
  .input-bar {
    border-top: 1px solid var(--border);
    background: var(--bg-elevated);
    flex-shrink: 0;
    position: relative;
    padding-bottom: var(--safe-bottom);
  }
  .input-bar.drag-over {
    border-color: var(--accent);
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
    padding: 4px 4px 4px 8px;
    transition: border-color 0.15s;
  }
  .input-wrapper:focus-within {
    border-color: var(--accent);
  }
  .input-wrapper.streaming {
    border-color: var(--border);
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
  .attach-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    width: 36px;
    height: 36px;
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    transition: color 0.15s, background 0.15s;
    align-self: flex-end;
    margin-bottom: 2px;
    cursor: pointer;
  }
  .attach-btn:hover {
    color: var(--accent);
    background: rgba(88, 166, 255, 0.1);
  }
  .stop-btn {
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid rgba(248, 81, 73, 0.3);
    color: var(--red);
    width: 36px;
    height: 36px;
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    transition: background 0.15s;
    align-self: flex-end;
    margin-bottom: 2px;
  }
  .stop-btn:hover {
    background: rgba(248, 81, 73, 0.2);
  }
  .stop-icon {
    font-size: 10px;
    line-height: 1;
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
  .send-btn.queuing {
    background: var(--yellow);
  }
  .send-btn.queuing:hover {
    background: #e0a820;
  }
  .queued-indicator {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 12px;
    margin-bottom: 6px;
    background: rgba(210, 153, 34, 0.1);
    border: 1px solid rgba(210, 153, 34, 0.3);
    border-radius: var(--radius-sm);
    font-size: 12px;
    color: var(--yellow);
  }
  .queued-text {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .queued-cancel {
    background: none;
    border: none;
    color: var(--yellow);
    font-size: 16px;
    padding: 0 4px;
    opacity: 0.7;
    line-height: 1;
  }
  .queued-cancel:hover {
    opacity: 1;
  }
  .attachment-preview {
    display: flex;
    gap: 8px;
    padding: 8px 0;
    flex-wrap: wrap;
  }
  .attachment-thumb {
    position: relative;
    width: 80px;
    height: 80px;
    border-radius: var(--radius-sm);
    overflow: hidden;
    border: 1px solid var(--border);
    background: var(--surface);
  }
  .attachment-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .file-icon {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--surface);
  }
  .file-emoji {
    font-size: 28px;
  }
  .attachment-thumb .remove-btn {
    position: absolute;
    top: 2px;
    right: 2px;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: rgba(0, 0, 0, 0.7);
    border: none;
    color: #fff;
    font-size: 14px;
    line-height: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.15s;
  }
  .attachment-thumb:hover .remove-btn {
    opacity: 1;
  }
  .attachment-thumb .attachment-name {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    padding: 2px 4px;
    background: rgba(0, 0, 0, 0.7);
    color: #fff;
    font-size: 9px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hidden-file-input {
    position: absolute;
    width: 0;
    height: 0;
    overflow: hidden;
    opacity: 0;
    pointer-events: none;
  }
  .drag-overlay {
    position: absolute;
    inset: 0;
    background: rgba(88, 166, 255, 0.08);
    border: 2px dashed var(--accent);
    border-radius: var(--radius);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 30;
    pointer-events: none;
  }
  .drag-label {
    color: var(--accent);
    font-size: 14px;
    font-weight: 500;
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

  @media (max-width: 768px) {
    .input-area {
      padding: 8px 10px;
    }
    .input-wrapper {
      padding: 2px 2px 2px 6px;
    }
    textarea {
      font-size: 16px; /* Prevents iOS zoom on focus */
      min-height: 36px;
      padding: 6px 0;
    }
    .send-btn {
      padding: 8px 12px;
      font-size: 13px;
    }
    .attach-btn {
      width: 32px;
      height: 32px;
    }
    .stop-btn {
      width: 32px;
      height: 32px;
    }
    .attachment-thumb {
      width: 64px;
      height: 64px;
    }
    .slash-menu {
      left: 10px;
      right: 10px;
    }
    .slash-item {
      padding: 10px 12px;
    }
  }
</style>
