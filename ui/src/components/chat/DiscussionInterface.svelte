<script lang="ts">
  interface DiscussionData {
    questions: DiscussionQuestion[];
    currentPhaseId?: string;
  }

  interface DiscussionQuestion {
    id: string;
    phaseId: string;
    question: string;
    options: Array<{ label: string; rationale: string }>;
    recommendation: string;
    answered?: boolean;
    decision?: string;
    userNote?: string;
  }

  let { 
    discussion, 
    onAnswer 
  }: {
    discussion: DiscussionData;
    onAnswer: (questionId: string, decision: string, userNote?: string) => Promise<void>;
  } = $props();

  let selectedAnswers = $state<Record<string, string>>({});
  let userNotes = $state<Record<string, string>>({});
  let submittingQuestion = $state<string | null>(null);
  let expandedQuestions = $state<Set<string>>(new Set());

  function toggleExpanded(questionId: string) {
    const newExpanded = new Set(expandedQuestions);
    if (newExpanded.has(questionId)) {
      newExpanded.delete(questionId);
    } else {
      newExpanded.add(questionId);
    }
    expandedQuestions = newExpanded;
  }

  function selectOption(questionId: string, optionLabel: string) {
    selectedAnswers = { ...selectedAnswers, [questionId]: optionLabel };
  }

  function updateNote(questionId: string, note: string) {
    userNotes = { ...userNotes, [questionId]: note };
  }

  async function submitAnswer(questionId: string) {
    const decision = selectedAnswers[questionId];
    if (!decision) return;

    submittingQuestion = questionId;
    try {
      await onAnswer(questionId, decision, userNotes[questionId]);
      // Clear local state
      delete selectedAnswers[questionId];
      delete userNotes[questionId];
    } catch (err) {
      console.error("Failed to submit answer:", err);
    } finally {
      submittingQuestion = null;
    }
  }

  let pendingQuestions = $derived(discussion.questions.filter(q => !q.answered));
  let answeredQuestions = $derived(discussion.questions.filter(q => q.answered));
</script>

<div class="discussion-interface">
  <div class="discussion-header">
    <h3>Discussion</h3>
    <div class="discussion-stats">
      <span class="stat pending">{pendingQuestions.length} pending</span>
      <span class="stat resolved">{answeredQuestions.length} resolved</span>
    </div>
  </div>

  {#if discussion.questions.length === 0}
    <div class="empty-state">
      <span class="empty-icon">💬</span>
      <p>No discussion questions yet</p>
    </div>
  {:else}
    <div class="discussion-content">
      <!-- Pending questions -->
      {#if pendingQuestions.length > 0}
        <div class="questions-section">
          <h4 class="section-title">Pending Questions</h4>
          {#each pendingQuestions as question (question.id)}
            <div class="question-card pending">
              <div class="question-header" onclick={() => toggleExpanded(question.id)}>
                <div class="question-text">
                  <h5>{question.question}</h5>
                  {#if question.recommendation}
                    <div class="recommendation">
                      <strong>Recommended:</strong> {question.recommendation}
                    </div>
                  {/if}
                </div>
                <span class="expand-icon" class:rotated={expandedQuestions.has(question.id)}>
                  ▼
                </span>
              </div>

              {#if expandedQuestions.has(question.id) || pendingQuestions.length <= 3}
                <div class="question-body">
                  <div class="options-list">
                    {#each question.options as option}
                      <label class="option-item">
                        <input
                          type="radio"
                          name="question-{question.id}"
                          value={option.label}
                          checked={selectedAnswers[question.id] === option.label}
                          onchange={() => selectOption(question.id, option.label)}
                        />
                        <div class="option-content">
                          <span class="option-label">{option.label}</span>
                          <p class="option-rationale">{option.rationale}</p>
                        </div>
                      </label>
                    {/each}
                  </div>

                  <div class="question-actions">
                    <textarea
                      class="note-input"
                      placeholder="Optional note explaining your decision..."
                      value={userNotes[question.id] || ""}
                      oninput={(e) => updateNote(question.id, e.currentTarget.value)}
                    ></textarea>
                    
                    <button
                      class="submit-btn"
                      class:loading={submittingQuestion === question.id}
                      disabled={!selectedAnswers[question.id] || submittingQuestion === question.id}
                      onclick={() => submitAnswer(question.id)}
                    >
                      {#if submittingQuestion === question.id}
                        <span class="loading-spinner">⟳</span>
                        Submitting...
                      {:else}
                        Submit Decision
                      {/if}
                    </button>
                  </div>
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}

      <!-- Resolved questions -->
      {#if answeredQuestions.length > 0}
        <div class="questions-section">
          <h4 class="section-title">Resolved Questions</h4>
          {#each answeredQuestions as question (question.id)}
            <div class="question-card resolved">
              <div class="question-header" onclick={() => toggleExpanded(question.id)}>
                <div class="question-text">
                  <h5>{question.question}</h5>
                  <div class="decision">
                    <strong>Decision:</strong> {question.decision}
                  </div>
                </div>
                <span class="expand-icon" class:rotated={expandedQuestions.has(question.id)}>
                  ▼
                </span>
              </div>

              {#if expandedQuestions.has(question.id)}
                <div class="question-body">
                  {#if question.userNote}
                    <div class="user-note">
                      <strong>Note:</strong>
                      <p>{question.userNote}</p>
                    </div>
                  {/if}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .discussion-interface {
    background: var(--bg);
    border-radius: var(--radius-sm);
  }

  .discussion-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 20px;
    flex-wrap: wrap;
    gap: 8px;
  }

  .discussion-header h3 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--text);
  }

  .discussion-stats {
    display: flex;
    gap: 12px;
    align-items: center;
  }

  .stat {
    font-size: var(--text-xs);
    font-weight: 600;
    padding: 3px 8px;
    border-radius: var(--radius-pill);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .stat.pending {
    color: var(--accent);
    background: rgba(154, 123, 79, 0.1);
    border: 1px solid rgba(154, 123, 79, 0.3);
  }

  .stat.resolved {
    color: var(--status-success);
    background: var(--status-success-bg);
    border: 1px solid var(--status-success-border);
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 48px 24px;
    text-align: center;
    color: var(--text-muted);
  }

  .empty-icon {
    font-size: 2rem;
    margin-bottom: 8px;
  }

  .empty-state p {
    margin: 0;
    font-size: var(--text-sm);
  }

  .discussion-content {
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .questions-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .section-title {
    margin: 0;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .question-card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .question-card.pending {
    border-left: 4px solid var(--accent);
  }

  .question-card.resolved {
    border-left: 4px solid var(--status-success);
    opacity: 0.8;
  }

  .question-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px;
    cursor: pointer;
    transition: background var(--transition-quick);
  }

  .question-header:hover {
    background: var(--surface-hover);
  }

  .question-text {
    flex: 1;
    min-width: 0;
  }

  .question-text h5 {
    margin: 0 0 6px 0;
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--text);
    line-height: 1.4;
  }

  .recommendation,
  .decision {
    font-size: var(--text-xs);
    color: var(--text-muted);
    margin-top: 4px;
  }

  .recommendation strong,
  .decision strong {
    color: var(--text-secondary);
  }

  .expand-icon {
    font-size: var(--text-xs);
    color: var(--text-muted);
    transition: transform var(--transition-quick);
    transform: rotate(-90deg);
    flex-shrink: 0;
    margin-left: 8px;
  }

  .expand-icon.rotated {
    transform: rotate(0deg);
  }

  .question-body {
    padding: 0 16px 16px 16px;
    background: var(--bg-elevated);
    border-top: 1px solid var(--border);
  }

  .options-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 16px;
  }

  .option-item {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 12px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: all var(--transition-quick);
  }

  .option-item:hover {
    border-color: var(--accent);
    background: var(--surface-hover);
  }

  .option-item:has(input:checked) {
    border-color: var(--accent);
    background: rgba(154, 123, 79, 0.05);
  }

  .option-item input[type="radio"] {
    margin: 0;
    flex-shrink: 0;
  }

  .option-content {
    flex: 1;
    min-width: 0;
  }

  .option-label {
    display: block;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text);
    margin-bottom: 4px;
    line-height: 1.3;
  }

  .option-rationale {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--text-muted);
    line-height: 1.4;
  }

  .question-actions {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .note-input {
    width: 100%;
    min-height: 60px;
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text);
    font-size: var(--text-sm);
    font-family: var(--font-sans);
    resize: vertical;
  }

  .note-input:focus {
    outline: none;
    border-color: var(--accent);
  }

  .note-input::placeholder {
    color: var(--text-muted);
  }

  .submit-btn {
    align-self: flex-end;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 16px;
    background: var(--accent);
    color: white;
    border: none;
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
    transition: background var(--transition-quick);
  }

  .submit-btn:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .submit-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .loading-spinner {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .user-note {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 12px;
  }

  .user-note strong {
    color: var(--text-secondary);
    font-size: var(--text-xs);
    display: block;
    margin-bottom: 6px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .user-note p {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--text-muted);
    line-height: 1.4;
  }

  @media (max-width: 768px) {
    .discussion-header {
      flex-direction: column;
      align-items: stretch;
    }

    .discussion-stats {
      justify-content: center;
    }

    .question-header {
      flex-direction: column;
      align-items: stretch;
      gap: 8px;
    }

    .expand-icon {
      align-self: flex-end;
      margin-left: 0;
    }

    .submit-btn {
      align-self: stretch;
      justify-content: center;
    }
  }
</style>