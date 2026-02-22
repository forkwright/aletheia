<script lang="ts">
  import type { PendingApproval } from "../../lib/types";
  import { approveToolCall, denyToolCall } from "../../lib/api";
  import Spinner from "../shared/Spinner.svelte";

  let { approval, onResolved }: {
    approval: PendingApproval;
    onResolved: () => void;
  } = $props();

  let resolving = $state(false);
  let alwaysAllow = $state(false);
  let error = $state<string | null>(null);

  function humanize(name: string): string {
    switch (name) {
      case "exec": return "Run command";
      case "file_write": return "Write file";
      case "file_edit": return "Edit file";
      case "message": return "Send message";
      case "voice_reply": return "Voice reply";
      case "fact_retract": return "Delete memory";
      case "sessions_send": return "Message agent";
      default: return name;
    }
  }

  function riskLabel(risk: string): string {
    switch (risk) {
      case "destructive": return "Destructive";
      case "irreversible": return "Irreversible";
      default: return "Risky";
    }
  }

  function formatInput(input: unknown): string {
    if (!input || typeof input !== "object") return String(input ?? "");
    const obj = input as Record<string, unknown>;

    // Special formatting for exec commands
    if ("command" in obj) return String(obj["command"]);

    // Special formatting for file ops
    if ("path" in obj) {
      const path = String(obj["path"]);
      if ("content" in obj) {
        const content = String(obj["content"]);
        return `${path}\n${content.length > 500 ? content.slice(0, 500) + "..." : content}`;
      }
      return path;
    }

    // Generic JSON
    try {
      return JSON.stringify(input, null, 2).slice(0, 1000);
    } catch {
      return String(input);
    }
  }

  async function handleApprove() {
    resolving = true;
    error = null;
    try {
      await approveToolCall(approval.turnId, approval.toolId, alwaysAllow);
      onResolved();
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
      resolving = false;
    }
  }

  async function handleDeny() {
    resolving = true;
    error = null;
    try {
      await denyToolCall(approval.turnId, approval.toolId);
      onResolved();
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
      resolving = false;
    }
  }
</script>

<div class="approval-card">
  <div class="approval-header">
    <span class="approval-icon">
      {#if approval.risk === "destructive"}
        <span class="risk-badge destructive">{riskLabel(approval.risk)}</span>
      {:else}
        <span class="risk-badge irreversible">{riskLabel(approval.risk)}</span>
      {/if}
    </span>
    <span class="approval-title">Approve {humanize(approval.toolName)}?</span>
  </div>

  <div class="approval-reason">{approval.reason}</div>

  <pre class="approval-input">{formatInput(approval.input)}</pre>

  {#if error}
    <div class="approval-error">{error}</div>
  {/if}

  <div class="approval-actions">
    <label class="always-allow">
      <input type="checkbox" bind:checked={alwaysAllow} disabled={resolving} />
      Always allow <code>{approval.toolName}</code>
    </label>
    <div class="approval-buttons">
      <button class="btn-deny" onclick={handleDeny} disabled={resolving}>
        {#if resolving}
          <Spinner size={12} />
        {:else}
          Deny
        {/if}
      </button>
      <button class="btn-approve" onclick={handleApprove} disabled={resolving}>
        {#if resolving}
          <Spinner size={12} />
        {:else}
          Approve
        {/if}
      </button>
    </div>
  </div>
</div>

<style>
  .approval-card {
    margin: 8px 0;
    padding: 12px 16px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-left: 3px solid var(--yellow, #d29922);
    border-radius: var(--radius-sm);
    animation: fade-in 0.2s ease;
  }
  @keyframes fade-in {
    from { opacity: 0; transform: translateY(4px); }
    to { opacity: 1; transform: translateY(0); }
  }
  .approval-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }
  .approval-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text);
  }
  .risk-badge {
    display: inline-block;
    font-size: var(--text-2xs);
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
  }
  .risk-badge.destructive {
    background: rgba(248, 81, 73, 0.15);
    color: var(--red, #f85149);
  }
  .risk-badge.irreversible {
    background: rgba(210, 153, 34, 0.15);
    color: var(--yellow, #d29922);
  }
  .approval-reason {
    font-size: var(--text-sm);
    color: var(--text-secondary);
    margin-bottom: 8px;
  }
  .approval-input {
    margin: 0 0 10px;
    padding: 8px 10px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-all;
    color: var(--text);
    max-height: 200px;
    overflow: auto;
  }
  .approval-error {
    font-size: var(--text-xs);
    color: var(--status-error);
    margin-bottom: 6px;
  }
  .approval-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .always-allow {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: var(--text-xs);
    color: var(--text-muted);
    cursor: pointer;
  }
  .always-allow input[type="checkbox"] {
    margin: 0;
  }
  .always-allow code {
    font-family: var(--font-mono);
    font-size: var(--text-2xs);
    background: var(--surface);
    padding: 1px 4px;
    border-radius: var(--radius-sm);
  }
  .approval-buttons {
    display: flex;
    gap: 8px;
  }
  .btn-approve, .btn-deny {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 5px 14px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
    transition: background var(--transition-quick), border-color var(--transition-quick);
  }
  .btn-approve {
    background: rgba(63, 185, 80, 0.15);
    color: var(--green, #3fb950);
    border-color: rgba(63, 185, 80, 0.3);
  }
  .btn-approve:hover:not(:disabled) {
    background: rgba(63, 185, 80, 0.25);
    border-color: var(--green, #3fb950);
  }
  .btn-deny {
    background: rgba(248, 81, 73, 0.1);
    color: var(--red, #f85149);
    border-color: rgba(248, 81, 73, 0.2);
  }
  .btn-deny:hover:not(:disabled) {
    background: rgba(248, 81, 73, 0.2);
    border-color: var(--red, #f85149);
  }
  .btn-approve:disabled, .btn-deny:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  @media (max-width: 768px) {
    .approval-card {
      margin: 6px 0;
      padding: 10px 12px;
    }
    .approval-actions {
      flex-direction: column;
      gap: 8px;
      align-items: stretch;
    }
    .approval-buttons {
      justify-content: stretch;
    }
    .btn-approve, .btn-deny {
      flex: 1;
      justify-content: center;
      padding: 8px 14px;
    }
    .approval-input {
      font-size: var(--text-xs);
      max-height: 150px;
    }
  }
</style>
