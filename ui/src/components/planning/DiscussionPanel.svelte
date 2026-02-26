<script lang="ts">
  import Spinner from "../shared/Spinner.svelte";
  import { authFetch } from "./api";

  interface DiscussionOption {
    label: string;
    rationale: string;
  }

  interface DiscussionQuestion {
    id: string;
    question: string;
    description?: string | undefined;
    options: DiscussionOption[];
    recommendation?: string | undefined;
    answered: boolean;
    decision?: string | undefined;
    userNote?: string | undefined;
    answeredAt?: string | undefined;
  }

  let { projectId, phaseId }: { projectId: string; phaseId?: string } = $props();

  let questions = $state<DiscussionQuestion[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let submitting = $state<Record<string, boolean>>({});

  async function loadQuestions() {
    if (!projectId) return;

    try {
      loading = true;
      error = null;

      // In a real implementation, this would call the API endpoint defined in Spec 32
      // For now, we'll mock some sample questions
      const url = phaseId 
        ? `/api/planning/projects/${projectId}/discuss?phaseId=${encodeURIComponent(phaseId)}`
        : `/api/planning/projects/${projectId}/discuss`;
      const res = await authFetch(url);
      
      if (!res.ok) {
        const errData = await res.json().catch(() => ({})) as { error?: string };
        error = errData.error || `Failed to load questions (${res.status})`;
        return;
      }

      const data = await res.json() as { questions?: Array<{
        id: string;
        question: string;
        description?: string;
        options: DiscussionOption[];
        recommendation?: string;
        status: string;
        decision?: string | null;
        userNote?: string | null;
        updatedAt?: string;
      }> };
      questions = (data.questions ?? []).map(q => ({
        id: q.id,
        question: q.question,
        description: q.description,
        options: q.options,
        recommendation: q.recommendation,
        answered: q.status === "answered" || q.status === "skipped",
        decision: q.decision ?? undefined,
        userNote: q.userNote ?? undefined,
        answeredAt: q.status !== "pending" ? q.updatedAt : undefined,
      }));
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  }

  async function submitDecision(questionId: string, decision: string, userNote?: string) {
    if (submitting[questionId]) return;

    try {
      submitting[questionId] = true;
      
      const postUrl = phaseId
        ? `/api/planning/projects/${projectId}/discuss?phaseId=${encodeURIComponent(phaseId)}`
        : `/api/planning/projects/${projectId}/discuss`;
      const res = await authFetch(postUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          questionId,
          decision,
          userNote: userNote?.trim() || undefined
        })
      });

      if (!res.ok) {
        throw new Error('Failed to submit decision');
      }

      // Update the question locally
      questions = questions.map(q => 
        q.id === questionId 
          ? { 
              ...q, 
              answered: true, 
              decision, 
              userNote: userNote?.trim() || undefined,
              answeredAt: new Date().toISOString()
            }
          : q
      );
      
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      submitting[questionId] = false;
    }
  }

  // Load questions when component mounts or projectId changes
  $effect(() => {
    loadQuestions();
  });

  let pendingQuestions = $derived.by(() => questions.filter(q => !q.answered));
  let answeredQuestions = $derived.by(() => questions.filter(q => q.answered));
  let showAnswered = $state(false);

  function handleOptionSelect(questionId: string, option: DiscussionOption, customNote?: string) {
    submitDecision(questionId, option.label, customNote);
  }

  function handleCustomDecision(questionId: string, customDecision: string, customNote?: string) {
    submitDecision(questionId, customDecision, customNote);
  }
</script>

<div class="discussion-section">
  <div class="section-header">
    <h2 class="section-title">
      <span class="title-icon">💬</span>
      Phase Discussion
      {#if pendingQuestions.length > 0}
        <span class="pending-count">{pendingQuestions.length} pending</span>
      {/if}
    </h2>
    
    {#if answeredQuestions.length > 0}
      <button 
        class="toggle-answered"
        onclick={() => showAnswered = !showAnswered}
      >
        {showAnswered ? "Hide" : "Show"} Answered ({answeredQuestions.length})
      </button>
    {/if}
  </div>

  <div class="discussion-container">
    {#if loading}
      <div class="loading-state">
        <Spinner size={20} />
        <span>Loading discussion questions...</span>
      </div>
    {:else if error}
      <div class="error-state">
        <span class="error-icon">⚠️</span>
        <span>Error loading questions: {error}</span>
        <button onclick={loadQuestions}>Retry</button>
      </div>
    {:else if questions.length === 0}
      <div class="empty-state">
        <span class="empty-icon">✅</span>
        <span>No discussion questions for this phase</span>
      </div>
    {:else}
      <div class="questions-list">
        <!-- Pending Questions -->
        {#if pendingQuestions.length > 0}
          <div class="question-group">
            <h3 class="group-title">Questions Requiring Decision</h3>
            {#each pendingQuestions as question (question.id)}
              {@render QuestionCard(question, submitting[question.id] || false, false, (option, note) => handleOptionSelect(question.id, option, note), (decision, note) => handleCustomDecision(question.id, decision, note))}
            {/each}
          </div>
        {/if}

        <!-- Answered Questions -->
        {#if showAnswered && answeredQuestions.length > 0}
          <div class="question-group">
            <h3 class="group-title">Answered Questions</h3>
            {#each answeredQuestions as question (question.id)}
              {@render QuestionCard(question, false, true)}
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<!-- Question Card Component -->
{#snippet QuestionCard(question: DiscussionQuestion, isSubmitting: boolean, readonly = false, onSelectOption?: (option: DiscussionOption, note?: string) => void, onCustomDecision?: (decision: string, note?: string) => void)}
  <div class="question-card" class:answered={question.answered}>
    <div class="question-header">
      <h4 class="question-title">{question.question}</h4>
      {#if question.answered}
        <span class="answered-badge">✓ Answered</span>
      {:else if question.recommendation}
        <span class="recommended-badge">⭐ Recommended</span>
      {/if}
    </div>

    {#if question.description}
      <p class="question-description">{question.description}</p>
    {/if}

    {#if question.answered}
      <!-- Show decision -->
      <div class="decision-display">
        <div class="decision-header">
          <strong>Decision:</strong>
          <span class="decision-text">{question.decision}</span>
        </div>
        {#if question.userNote}
          <div class="decision-note">
            <strong>Note:</strong>
            <span>{question.userNote}</span>
          </div>
        {/if}
        {#if question.answeredAt}
          <div class="decision-time">
            Decided {new Date(question.answeredAt).toLocaleString()}
          </div>
        {/if}
      </div>
    {:else if !readonly}
      <!-- Show options -->
      <div class="question-options">
        {#each question.options as option (option.label)}
          <div class="option-card" class:recommended={option.label === question.recommendation}>
            <div class="option-header">
              <span class="option-label">{option.label}</span>
              {#if option.label === question.recommendation}
                <span class="rec-badge">Recommended</span>
              {/if}
            </div>
            <p class="option-rationale">{option.rationale}</p>
            <button 
              class="select-option-btn"
              onclick={() => onSelectOption?.(option)}
              disabled={isSubmitting}
            >
              {#if isSubmitting}
                <Spinner size={12} />
              {:else}
                Select
              {/if}
            </button>
          </div>
        {/each}

        <!-- Custom decision option -->
        <details class="custom-option">
          <summary>Custom Decision</summary>
          <div class="custom-form">
            <input
              type="text"
              placeholder="Enter custom decision..."
              class="custom-input"
              onkeydown={(e) => {
                const input = e.currentTarget;
                if (e.key === 'Enter' && input.value.trim()) {
                  onCustomDecision?.(input.value.trim());
                  input.value = '';
                }
              }}
            />
            <textarea
              placeholder="Optional note explaining the decision..."
              class="custom-note"
              rows="2"
            ></textarea>
            <button
              class="submit-custom-btn"
              onclick={(e) => {
                const form = e.currentTarget.closest('.custom-form');
                const input = form?.querySelector<HTMLInputElement>('.custom-input');
                const note = form?.querySelector<HTMLTextAreaElement>('.custom-note');
                if (input?.value.trim()) {
                  onCustomDecision?.(input.value.trim(), note?.value.trim() || undefined);
                  input.value = '';
                  if (note) note.value = '';
                }
              }}
              disabled={isSubmitting}
            >
              Submit Custom Decision
            </button>
          </div>
        </details>
      </div>
    {/if}
  </div>
{/snippet}

<style>
  .discussion-section {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-3);
  }

  .section-title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--text);
    margin: 0;
  }

  .title-icon {
    font-size: var(--text-xl);
  }

  .pending-count {
    background: var(--status-warning);
    color: white;
    font-size: var(--text-xs);
    font-weight: 600;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-pill);
  }

  .toggle-answered {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-2);
    cursor: pointer;
    transition: all var(--transition-quick);
  }

  .toggle-answered:hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  .discussion-container {
    flex: 1;
    overflow-y: auto;
  }

  .loading-state,
  .error-state,
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    padding: var(--space-6);
    color: var(--text-muted);
    font-size: var(--text-sm);
  }

  .error-state {
    color: var(--status-error);
  }

  .error-state button {
    background: var(--status-error);
    color: white;
    border: none;
    border-radius: var(--radius-sm);
    padding: var(--space-1) var(--space-3);
    font-size: var(--text-xs);
    cursor: pointer;
    margin-top: var(--space-2);
  }

  .question-group {
    margin-bottom: var(--space-4);
  }

  .group-title {
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--text);
    margin: 0 0 var(--space-3) 0;
    border-bottom: 1px solid var(--border);
    padding-bottom: var(--space-1);
  }

  .question-card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-4);
    margin-bottom: var(--space-3);
    transition: border-color var(--transition-quick);
  }

  .question-card.answered {
    background: color-mix(in srgb, var(--status-success) 5%, transparent);
    border-color: color-mix(in srgb, var(--status-success) 20%, transparent);
  }

  .question-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }

  .question-title {
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--text);
    margin: 0;
    line-height: 1.3;
  }

  .answered-badge {
    background: var(--status-success);
    color: white;
    font-size: var(--text-xs);
    font-weight: 600;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-pill);
    white-space: nowrap;
  }

  .recommended-badge {
    background: var(--status-warning);
    color: white;
    font-size: var(--text-xs);
    font-weight: 600;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-pill);
    white-space: nowrap;
  }

  .question-description {
    color: var(--text-secondary);
    font-size: var(--text-sm);
    line-height: 1.5;
    margin: 0 0 var(--space-3) 0;
  }

  .decision-display {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--space-3);
  }

  .decision-header {
    display: flex;
    gap: var(--space-2);
    margin-bottom: var(--space-1);
  }

  .decision-text {
    color: var(--text);
    font-weight: 600;
  }

  .decision-note {
    display: flex;
    gap: var(--space-2);
    margin-bottom: var(--space-1);
    font-size: var(--text-sm);
  }

  .decision-time {
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .question-options {
    display: grid;
    gap: var(--space-3);
    grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
  }

  .option-card {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--space-3);
    transition: border-color var(--transition-quick);
  }

  .option-card.recommended {
    border-color: var(--status-warning);
    background: color-mix(in srgb, var(--status-warning) 5%, transparent);
  }

  .option-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-2);
  }

  .option-label {
    font-weight: 600;
    color: var(--text);
  }

  .rec-badge {
    background: var(--status-warning);
    color: white;
    font-size: var(--text-2xs);
    font-weight: 600;
    padding: 2px var(--space-1);
    border-radius: var(--radius-pill);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .option-rationale {
    color: var(--text-secondary);
    font-size: var(--text-sm);
    line-height: 1.4;
    margin: 0 0 var(--space-3) 0;
  }

  .select-option-btn {
    width: 100%;
    background: var(--accent);
    color: white;
    border: none;
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
    transition: background var(--transition-quick);
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
  }

  .select-option-btn:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .select-option-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .custom-option {
    grid-column: 1 / -1;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    margin-top: var(--space-2);
  }

  .custom-option summary {
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    font-weight: 600;
    color: var(--text-secondary);
    background: var(--surface);
    border-radius: var(--radius-sm);
  }

  .custom-option[open] summary {
    border-bottom: 1px solid var(--border);
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
  }

  .custom-form {
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .custom-input,
  .custom-note {
    width: 100%;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--space-2);
    color: var(--text);
    font-size: var(--text-sm);
  }

  .custom-input:focus,
  .custom-note:focus {
    outline: none;
    border-color: var(--accent);
  }

  .submit-custom-btn {
    background: var(--accent);
    color: white;
    border: none;
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
    align-self: flex-start;
  }

  .submit-custom-btn:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  @media (max-width: 768px) {
    .question-options {
      grid-template-columns: 1fr;
    }

    .section-header {
      flex-direction: column;
      align-items: flex-start;
      gap: var(--space-2);
    }

    .question-header {
      flex-direction: column;
      align-items: flex-start;
    }
  }
</style>