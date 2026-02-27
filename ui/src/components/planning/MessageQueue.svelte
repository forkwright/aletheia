<script lang="ts">
  /**
   * MessageQueue — Inject messages into running plan execution (INTERJ-01/02)
   *
   * Shows pending/delivered messages and provides a form to inject new ones.
   * Critical messages pause execution at the next turn boundary.
   */
  import { onMount, onDestroy } from "svelte";

  interface Props {
    projectId: string;
  }

  interface PlanningMessage {
    id: string;
    projectId: string;
    phaseId: string | null;
    source: "user" | "agent" | "sub-agent" | "system";
    sourceSessionId: string | null;
    content: string;
    priority: "low" | "normal" | "high" | "critical";
    status: "pending" | "delivered" | "expired";
    deliveredAt: string | null;
    expiresAt: string | null;
    createdAt: string;
  }

  let { projectId }: Props = $props();

  let messages = $state<PlanningMessage[]>([]);
  let pendingCount = $state(0);
  let newContent = $state("");
  let newPriority = $state<"low" | "normal" | "high" | "critical">("normal");
  let sending = $state(false);
  let error = $state<string | null>(null);
  let expanded = $state(false);
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  const priorityColors: Record<string, string> = {
    critical: "#dc2626",
    high: "#ea580c",
    normal: "#2563eb",
    low: "#6b7280",
  };

  const priorityIcons: Record<string, string> = {
    critical: "🛑",
    high: "⚠️",
    normal: "💬",
    low: "📝",
  };

  const statusIcons: Record<string, string> = {
    pending: "⏳",
    delivered: "✅",
    expired: "⏰",
  };

  async function fetchMessages() {
    try {
      const res = await fetch(`/api/planning/projects/${projectId}/messages`);
      if (res.ok) {
        const data = await res.json();
        messages = data.messages ?? [];
        pendingCount = data.pendingCount ?? 0;
      }
    } catch {
      // Silently retry on next poll
    }
  }

  async function sendMessage() {
    if (!newContent.trim()) return;
    sending = true;
    error = null;

    try {
      const res = await fetch(`/api/planning/projects/${projectId}/messages`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          content: newContent.trim(),
          priority: newPriority,
          source: "user",
        }),
      });

      if (res.ok) {
        newContent = "";
        newPriority = "normal";
        await fetchMessages();
      } else {
        const data = await res.json();
        error = data.error ?? "Failed to send message";
      }
    } catch (e) {
      error = e instanceof Error ? e.message : "Network error";
    } finally {
      sending = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  }

  function timeAgo(dateStr: string): string {
    const diff = Date.now() - new Date(dateStr).getTime();
    const seconds = Math.floor(diff / 1000);
    if (seconds < 60) return `${seconds}s ago`;
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `${minutes}m ago`;
    const hours = Math.floor(minutes / 60);
    return `${hours}h ago`;
  }

  onMount(() => {
    fetchMessages();
    pollTimer = setInterval(fetchMessages, 5000);
  });

  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
  });
</script>

<div class="message-queue">
  <button class="header" onclick={() => expanded = !expanded}>
    <span class="title">
      💬 Messages
      {#if pendingCount > 0}
        <span class="badge pending">{pendingCount} pending</span>
      {/if}
    </span>
    <span class="chevron" class:open={expanded}>▸</span>
  </button>

  {#if expanded}
    <div class="body">
      <!-- Compose -->
      <div class="compose">
        <div class="compose-row">
          <textarea
            bind:value={newContent}
            placeholder="Inject a message into execution..."
            rows="2"
            onkeydown={handleKeydown}
            disabled={sending}
          ></textarea>
        </div>
        <div class="compose-controls">
          <select bind:value={newPriority} disabled={sending}>
            <option value="low">📝 Low</option>
            <option value="normal">💬 Normal</option>
            <option value="high">⚠️ High</option>
            <option value="critical">🛑 Critical (pauses execution)</option>
          </select>
          <button class="send-btn" onclick={sendMessage} disabled={sending || !newContent.trim()}>
            {sending ? "Sending..." : "Send"}
          </button>
        </div>
        {#if error}
          <div class="error">{error}</div>
        {/if}
      </div>

      <!-- Message List -->
      {#if messages.length === 0}
        <div class="empty">No messages yet</div>
      {:else}
        <div class="message-list">
          {#each messages as msg (msg.id)}
            <div class="message" style="border-left-color: {priorityColors[msg.priority]}">
              <div class="msg-header">
                <span class="msg-priority">{priorityIcons[msg.priority]}</span>
                <span class="msg-source">{msg.source}</span>
                <span class="msg-status">{statusIcons[msg.status]} {msg.status}</span>
                <span class="msg-time">{timeAgo(msg.createdAt)}</span>
              </div>
              <div class="msg-content">{msg.content}</div>
              {#if msg.deliveredAt}
                <div class="msg-meta">Delivered {timeAgo(msg.deliveredAt)}</div>
              {/if}
              {#if msg.sourceSessionId}
                <div class="msg-meta">From: {msg.sourceSessionId}</div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .message-queue {
    border: 1px solid var(--border-color, #333);
    border-radius: 6px;
    overflow: hidden;
    margin-top: 0.5rem;
  }

  .header {
    width: 100%;
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.5rem 0.75rem;
    background: var(--surface-color, #1a1a1a);
    border: none;
    color: inherit;
    cursor: pointer;
    font-size: 0.85rem;
  }

  .header:hover {
    background: var(--hover-color, #222);
  }

  .title {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-weight: 500;
  }

  .badge {
    font-size: 0.7rem;
    padding: 0.1rem 0.4rem;
    border-radius: 9999px;
    font-weight: 600;
  }

  .badge.pending {
    background: #2563eb30;
    color: #60a5fa;
  }

  .chevron {
    transition: transform 0.15s;
    font-size: 0.75rem;
    color: var(--muted-color, #888);
  }

  .chevron.open {
    transform: rotate(90deg);
  }

  .body {
    padding: 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .compose {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .compose-row textarea {
    width: 100%;
    background: var(--input-bg, #111);
    color: inherit;
    border: 1px solid var(--border-color, #333);
    border-radius: 4px;
    padding: 0.5rem;
    font-size: 0.8rem;
    font-family: inherit;
    resize: vertical;
    box-sizing: border-box;
  }

  .compose-controls {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }

  .compose-controls select {
    flex: 1;
    background: var(--input-bg, #111);
    color: inherit;
    border: 1px solid var(--border-color, #333);
    border-radius: 4px;
    padding: 0.3rem 0.5rem;
    font-size: 0.75rem;
  }

  .send-btn {
    padding: 0.3rem 0.75rem;
    background: #2563eb;
    color: white;
    border: none;
    border-radius: 4px;
    font-size: 0.75rem;
    cursor: pointer;
    white-space: nowrap;
  }

  .send-btn:hover:not(:disabled) {
    background: #1d4ed8;
  }

  .send-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .error {
    color: #ef4444;
    font-size: 0.75rem;
  }

  .empty {
    color: var(--muted-color, #888);
    font-size: 0.8rem;
    text-align: center;
    padding: 0.5rem;
  }

  .message-list {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    max-height: 300px;
    overflow-y: auto;
  }

  .message {
    border-left: 3px solid #333;
    padding: 0.4rem 0.6rem;
    background: var(--surface-color, #1a1a1a);
    border-radius: 0 4px 4px 0;
    font-size: 0.8rem;
  }

  .msg-header {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    font-size: 0.7rem;
    color: var(--muted-color, #888);
    margin-bottom: 0.2rem;
  }

  .msg-source {
    font-weight: 500;
    color: var(--text-color, #ccc);
  }

  .msg-content {
    color: var(--text-color, #ddd);
    white-space: pre-wrap;
    word-break: break-word;
  }

  .msg-meta {
    font-size: 0.65rem;
    color: var(--muted-color, #666);
    margin-top: 0.2rem;
  }
</style>
