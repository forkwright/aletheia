<script lang="ts">
  import type { PlanProposal } from "../../lib/types";
  import { approvePlan, cancelPlan } from "../../lib/api";
  import Spinner from "../shared/Spinner.svelte";

  let { plan, onResolved }: {
    plan: PlanProposal;
    onResolved: () => void;
  } = $props();

  let resolving = $state(false);
  let error = $state<string | null>(null);
  let skippedSteps = $state<Set<number>>(new Set());

  const roleIcon: Record<string, string> = {
    self: "🧠",
    coder: "💻",
    reviewer: "🔍",
    researcher: "🔬",
    explorer: "🗺️",
    runner: "⚡",
  };

  const roleLabel: Record<string, string> = {
    self: "Direct",
    coder: "Coder",
    reviewer: "Reviewer",
    researcher: "Researcher",
    explorer: "Explorer",
    runner: "Runner",
  };

  function toggleStep(stepId: number) {
    const next = new Set(skippedSteps);
    if (next.has(stepId)) next.delete(stepId);
    else next.add(stepId);
    skippedSteps = next;
  }

  function formatCost(cents: number): string {
    if (cents < 100) return `${cents}¢`;
    return `$${(cents / 100).toFixed(2)}`;
  }

  let approvedCount = $derived(plan.steps.length - skippedSteps.size);
  let estimatedCost = $derived(
    plan.steps
      .filter(s => !skippedSteps.has(s.id))
      .reduce((sum, s) => sum + s.estimatedCostCents, 0)
  );

  async function handleApprove() {
    resolving = true;
    error = null;
    try {
      await approvePlan(plan.id, skippedSteps.size > 0 ? [...skippedSteps] : undefined);
      onResolved();
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
      resolving = false;
    }
  }

  async function handleCancel() {
    resolving = true;
    error = null;
    try {
      await cancelPlan(plan.id);
      onResolved();
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
      resolving = false;
    }
  }
</script>

<div class="plan-card">
  <div class="plan-header">
    <span class="plan-icon">📋</span>
    <span class="plan-title">Plan Proposed</span>
    <span class="plan-cost">{formatCost(estimatedCost)} estimated</span>
  </div>

  <div class="plan-steps">
    {#each plan.steps as step (step.id)}
      <label class="plan-step" class:skipped={skippedSteps.has(step.id)}>
        <input
          type="checkbox"
          checked={!skippedSteps.has(step.id)}
          onchange={() => toggleStep(step.id)}
          disabled={resolving}
        />
        <span class="step-role" title={roleLabel[step.role] ?? step.role}>
          {roleIcon[step.role] ?? "⚙️"}
        </span>
        <span class="step-label">{step.label}</span>
        <span class="step-cost">{formatCost(step.estimatedCostCents)}</span>
      </label>
    {/each}
  </div>

  {#if error}
    <div class="plan-error">{error}</div>
  {/if}

  <div class="plan-actions">
    <span class="plan-summary">
      {approvedCount}/{plan.steps.length} steps selected
    </span>
    <div class="plan-buttons">
      <button class="btn-cancel" onclick={handleCancel} disabled={resolving}>
        {#if resolving}
          <Spinner size={12} />
        {:else}
          Cancel
        {/if}
      </button>
      <button class="btn-approve" onclick={handleApprove} disabled={resolving || approvedCount === 0}>
        {#if resolving}
          <Spinner size={12} />
        {:else}
          Approve {approvedCount > 0 ? `(${approvedCount})` : ""}
        {/if}
      </button>
    </div>
  </div>
</div>

<style>
  .plan-card {
    margin: 8px 0;
    padding: 12px 16px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-left: 3px solid var(--blue, #58a6ff);
    border-radius: var(--radius-sm);
    animation: fade-in 0.2s ease;
  }
  @keyframes fade-in {
    from { opacity: 0; transform: translateY(4px); }
    to { opacity: 1; transform: translateY(0); }
  }
  .plan-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 10px;
  }
  .plan-icon {
    font-size: 16px;
  }
  .plan-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
    flex: 1;
  }
  .plan-cost {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    background: var(--surface);
    padding: 2px 8px;
    border-radius: 10px;
  }
  .plan-steps {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 10px;
  }
  .plan-step {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background 0.1s;
    font-size: 13px;
  }
  .plan-step:hover {
    background: var(--surface);
  }
  .plan-step.skipped {
    opacity: 0.4;
  }
  .plan-step.skipped .step-label {
    text-decoration: line-through;
  }
  .plan-step input[type="checkbox"] {
    margin: 0;
    cursor: pointer;
  }
  .step-role {
    font-size: 14px;
    flex-shrink: 0;
    width: 20px;
    text-align: center;
  }
  .step-label {
    flex: 1;
    color: var(--text);
    line-height: 1.3;
  }
  .step-cost {
    font-size: 11px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    flex-shrink: 0;
  }
  .plan-error {
    font-size: 11px;
    color: var(--red);
    margin-bottom: 6px;
  }
  .plan-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .plan-summary {
    font-size: 11px;
    color: var(--text-muted);
  }
  .plan-buttons {
    display: flex;
    gap: 8px;
  }
  .btn-approve, .btn-cancel {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 5px 14px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.15s, border-color 0.15s;
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
  .btn-cancel {
    background: rgba(248, 81, 73, 0.1);
    color: var(--red, #f85149);
    border-color: rgba(248, 81, 73, 0.2);
  }
  .btn-cancel:hover:not(:disabled) {
    background: rgba(248, 81, 73, 0.2);
    border-color: var(--red, #f85149);
  }
  .btn-approve:disabled, .btn-cancel:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
