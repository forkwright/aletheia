<script lang="ts">
  interface DiscussionQuestion {
    id: string;
    question: string;
    options: Array<{ label: string; rationale: string }>;
    recommendation: string | null;
    decision: string | null;
    userNote: string | null;
    status: "pending" | "answered" | "skipped";
  }

  let { 
    question, 
    onSubmit 
  }: {
    question: DiscussionQuestion;
    onSubmit: (decision: string, userNote?: string) => void;
  } = $props();

  let selectedOption = $state<string | null>(null);
  let customAnswer = $state("");
  let userNote = $state("");
  let showCustom = $state(false);
  let submitting = $state(false);

  function handleOptionSelect(option: string): void {
    if (question.status !== "pending") return;
    selectedOption = option;
    showCustom = false;
    customAnswer = "";
  }

  function handleCustomToggle(): void {
    showCustom = !showCustom;
    if (showCustom) {
      selectedOption = null;
    } else {
      customAnswer = "";
    }
  }

  async function handleSubmit(): Promise<void> {
    if (submitting) return;
    
    const decision = showCustom && customAnswer.trim() 
      ? customAnswer.trim() 
      : selectedOption;
    
    if (!decision) return;
    
    submitting = true;
    try {
      await onSubmit(decision, userNote.trim() || undefined);
    } finally {
      submitting = false;
    }
  }

  function handleSkip(): void {
    if (submitting || question.status !== "pending") return;
    const recommendedOption = question.options.find(opt => 
      opt.label === question.recommendation
    )?.label;
    if (recommendedOption) {
      onSubmit(recommendedOption, "Skipped - using recommended option");
    }
  }

  let canSubmit = $derived(() => {
    if (question.status !== "pending" || submitting) return false;
    return (showCustom && customAnswer.trim()) || (!showCustom && selectedOption);
  });

  function getOptionStyle(optionLabel: string): string {
    if (question.status === "answered" && question.decision === optionLabel) {
      return "option-selected-final";
    }
    if (question.status === "pending" && selectedOption === optionLabel) {
      return "option-selected";
    }
    if (question.recommendation === optionLabel) {
      return "option-recommended";
    }
    return "";
  }
</script>

<div class="discussion-card">
  <div class="card-header">
    <div class="question-icon">
      {#if question.status === "answered"}
        ✓
      {:else if question.status === "skipped"}
        →
      {:else}
        🔶
      {/if}
    </div>
    <div class="question-text">
      <h5>{question.question}</h5>
      {#if question.status === "answered"}
        <div class="status-badge answered">Answered</div>
      {:else if question.status === "skipped"}
        <div class="status-badge skipped">Skipped</div>
      {:else}
        <div class="status-badge pending">Pending</div>
      {/if}
    </div>
  </div>

  <div class="card-body">
    {#if question.status === "answered"}
      <!-- Show final decision -->
      <div class="final-decision">
        <h6>Decision:</h6>
        <div class="decision-content">
          <p class="decision-text">{question.decision}</p>
          {#if question.userNote}
            <p class="decision-note">
              <strong>Note:</strong> {question.userNote}
            </p>
          {/if}
        </div>
      </div>
    {:else}
      <!-- Interactive options -->
      <div class="options-container">
        {#each question.options as option (option.label)}
          <button 
            class="option {getOptionStyle(option.label)}"
            class:disabled={question.status !== "pending"}
            onclick={() => handleOptionSelect(option.label)}
          >
            <div class="option-header">
              <div class="option-radio">
                {#if selectedOption === option.label}
                  ●
                {:else}
                  ○
                {/if}
              </div>
              <span class="option-label">{option.label}</span>
              {#if question.recommendation === option.label}
                <span class="recommended-badge">Recommended</span>
              {/if}
            </div>
            <p class="option-rationale">{option.rationale}</p>
          </button>
        {/each}

        <button 
          class="option custom-option"
          class:active={showCustom}
          class:disabled={question.status !== "pending"}
          onclick={handleCustomToggle}
        >
          <div class="option-header">
            <div class="option-radio">
              {#if showCustom}
                ●
              {:else}
                ○
              {/if}
            </div>
            <span class="option-label">Custom Response</span>
          </div>
          <p class="option-rationale">Provide your own answer</p>
        </button>

        {#if showCustom}
          <div class="custom-input">
            <textarea
              bind:value={customAnswer}
              placeholder="Enter your custom response..."
              disabled={question.status !== "pending"}
              rows="3"
            ></textarea>
          </div>
        {/if}

        {#if question.status === "pending"}
          <div class="note-input">
            <label for="user-note-{question.id}">Optional note:</label>
            <textarea
              id="user-note-{question.id}"
              bind:value={userNote}
              placeholder="Add any additional context or reasoning..."
              rows="2"
            ></textarea>
          </div>

          <div class="card-actions">
            <button 
              class="submit-btn"
              class:disabled={!canSubmit()}
              onclick={handleSubmit}
              disabled={!canSubmit()}
            >
              {#if submitting}
                <span class="spinner">⟳</span>
                Submitting...
              {:else}
                Confirm Decision
              {/if}
            </button>
            
            {#if question.recommendation}
              <button 
                class="skip-btn"
                onclick={handleSkip}
                disabled={submitting}
              >
                Skip (use recommended)
              </button>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .discussion-card {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: 20px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1);
  }

  .card-header {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    margin-bottom: 16px;
  }

  .question-icon {
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: rgba(154, 123, 79, 0.1);
    color: var(--accent);
    font-size: var(--text-base);
    flex-shrink: 0;
  }

  .question-text {
    flex: 1;
    min-width: 0;
  }

  .question-text h5 {
    margin: 0 0 8px 0;
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--text);
    line-height: 1.4;
  }

  .status-badge {
    display: inline-block;
    padding: 2px 8px;
    border-radius: var(--radius-pill);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .status-badge.pending {
    background: rgba(154, 123, 79, 0.1);
    color: var(--accent);
  }

  .status-badge.answered {
    background: var(--status-success-bg);
    color: var(--status-success);
  }

  .status-badge.skipped {
    background: var(--surface);
    color: var(--text-muted);
  }

  .card-body {
    margin-top: 16px;
  }

  .final-decision h6 {
    margin: 0 0 12px 0;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text-secondary);
  }

  .decision-content {
    background: var(--surface);
    border: 1px solid var(--border);
    border-left: 3px solid var(--status-success);
    border-radius: var(--radius-sm);
    padding: 12px;
  }

  .decision-text {
    margin: 0 0 8px 0;
    color: var(--text);
    font-weight: 500;
  }

  .decision-note {
    margin: 0;
    color: var(--text-secondary);
    font-size: var(--text-sm);
    font-style: italic;
  }

  .options-container {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .option {
    display: block;
    width: 100%;
    text-align: left;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 12px;
    cursor: pointer;
    transition: background var(--transition-quick), border-color var(--transition-quick);
  }

  .option:hover:not(.disabled) {
    background: var(--surface-hover);
  }

  .option.option-selected {
    border-color: var(--accent);
    background: rgba(154, 123, 79, 0.05);
  }

  .option.option-selected-final {
    border-color: var(--status-success);
    background: var(--status-success-bg);
  }

  .option.option-recommended:not(.option-selected):not(.option-selected-final) {
    border-left: 3px solid var(--accent);
  }

  .option.custom-option.active {
    border-color: var(--accent);
    background: rgba(154, 123, 79, 0.05);
  }

  .option.disabled {
    cursor: not-allowed;
    opacity: 0.6;
  }

  .option-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 4px;
  }

  .option-radio {
    width: 16px;
    height: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--accent);
    font-size: var(--text-sm);
    flex-shrink: 0;
  }

  .option-label {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--text);
    flex: 1;
  }

  .recommended-badge {
    background: rgba(154, 123, 79, 0.1);
    color: var(--accent);
    padding: 2px 6px;
    border-radius: var(--radius-pill);
    font-size: var(--text-2xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .option-rationale {
    margin: 0;
    color: var(--text-secondary);
    font-size: var(--text-sm);
    line-height: 1.4;
  }

  .custom-input {
    margin-top: 8px;
  }

  .custom-input textarea {
    width: 100%;
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text);
    font-family: inherit;
    font-size: var(--text-sm);
    resize: vertical;
    min-height: 60px;
  }

  .custom-input textarea:focus {
    outline: none;
    border-color: var(--accent);
  }

  .note-input {
    margin-top: 16px;
  }

  .note-input label {
    display: block;
    margin-bottom: 6px;
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .note-input textarea {
    width: 100%;
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text);
    font-family: inherit;
    font-size: var(--text-sm);
    resize: vertical;
  }

  .note-input textarea:focus {
    outline: none;
    border-color: var(--accent);
  }

  .card-actions {
    display: flex;
    gap: 12px;
    margin-top: 16px;
    justify-content: flex-end;
  }

  .submit-btn {
    padding: 10px 20px;
    background: var(--accent);
    color: white;
    border: none;
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 6px;
    transition: background var(--transition-quick);
  }

  .submit-btn:hover:not(.disabled) {
    background: var(--accent-hover);
  }

  .submit-btn.disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .skip-btn {
    padding: 10px 20px;
    background: var(--surface);
    color: var(--text-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-quick);
  }

  .skip-btn:hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  .spinner {
    animation: spin 1s linear infinite;
    display: inline-block;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  @media (max-width: 768px) {
    .discussion-card {
      padding: 16px;
    }

    .card-actions {
      flex-direction: column;
    }

    .submit-btn, .skip-btn {
      width: 100%;
      justify-content: center;
    }
  }
</style>