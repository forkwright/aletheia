<script lang="ts">
  import Spinner from "../shared/Spinner.svelte";
  import RequirementsTable from "./RequirementsTable.svelte";
  import RoadmapTimeline from "./RoadmapTimeline.svelte";
  import DiscussionCard from "./DiscussionCard.svelte";
  import ExecutionProgress from "./ExecutionProgress.svelte";

  interface ProjectData {
    id: string;
    goal: string;
    state: string;
    createdAt: string;
    updatedAt: string;
  }

  interface Requirement {
    id: string;
    reqId: string;
    description: string;
    category: string;
    tier: "v1" | "v2" | "out-of-scope";
    rationale: string | null;
    status: string;
  }

  interface Phase {
    id: string;
    name: string;
    goal: string;
    status: "pending" | "executing" | "complete" | "failed" | "skipped";
    phaseOrder: number;
    requirements: string[];
    successCriteria: string[];
  }

  interface Milestone {
    id: string;
    name: string;
    type: "builtin" | "phase";
    status: "pending" | "active" | "complete" | "failed";
    order: number;
    goal?: string;
    requirements?: string[];
    requirementCount?: number;
  }

  interface DiscussionQuestion {
    id: string;
    question: string;
    options: Array<{ label: string; rationale: string }>;
    recommendation: string | null;
    decision: string | null;
    userNote: string | null;
    status: "pending" | "answered" | "skipped";
  }

  let { projectId, onClose }: {
    projectId: string;
    onClose: () => void;
  } = $props();

  let activeTab = $state<"overview" | "requirements" | "roadmap" | "execution" | "discussion">("overview");
  let project = $state<ProjectData | null>(null);
  let requirements = $state<Requirement[]>([]);
  let phases = $state<Phase[]>([]);
  let timeline = $state<{ milestones: Milestone[]; requirementsSummary: any } | null>(null);
  let discussions = $state<DiscussionQuestion[]>([]);
  let execution = $state<any>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let currentPhaseId = $state<string | null>(null);

  async function fetchProjectData(): Promise<void> {
    if (!projectId) return;
    loading = true;
    error = null;

    try {
      // Fetch basic project info
      const projectRes = await fetch(`/api/planning/projects/${projectId}`);
      if (!projectRes.ok) {
        error = `Failed to load project: ${projectRes.status}`;
        return;
      }
      project = await projectRes.json();

      // Fetch all data in parallel
      const [requirementsRes, phasesRes, timelineRes, executionRes] = await Promise.all([
        fetch(`/api/planning/projects/${projectId}/requirements`).catch(() => null),
        fetch(`/api/planning/projects/${projectId}/phases`).catch(() => null),
        fetch(`/api/planning/projects/${projectId}/timeline`).catch(() => null),
        fetch(`/api/planning/projects/${projectId}/execution`).catch(() => null),
      ]);

      if (requirementsRes?.ok) {
        const reqData = await requirementsRes.json();
        requirements = reqData.requirements || [];
      }

      if (phasesRes?.ok) {
        const phasesData = await phasesRes.json();
        phases = phasesData.phases || [];
        
        // Find current phase for discussion loading
        currentPhaseId = phases.find(p => p.status === "executing")?.id || 
                        phases.find(p => p.status === "pending")?.id || 
                        null;
      }

      if (timelineRes?.ok) {
        timeline = await timelineRes.json();
      }

      if (executionRes?.ok) {
        execution = await executionRes.json();
      }

      // Load discussions for current phase if in discussing state
      if (project?.state === "discussing" && currentPhaseId) {
        await loadDiscussions(currentPhaseId);
      }

    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to load project data";
    } finally {
      loading = false;
    }
  }

  async function loadDiscussions(phaseId: string): Promise<void> {
    try {
      const res = await fetch(`/api/planning/projects/${projectId}/discuss?phaseId=${phaseId}`);
      if (res.ok) {
        const data = await res.json();
        discussions = data.questions || [];
      }
    } catch {
      // Ignore discussion loading errors
    }
  }

  async function submitDiscussion(questionId: string, decision: string, userNote?: string): Promise<void> {
    try {
      const res = await fetch(`/api/planning/projects/${projectId}/discuss`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ questionId, decision, userNote }),
      });
      
      if (res.ok) {
        // Refresh discussions
        if (currentPhaseId) {
          await loadDiscussions(currentPhaseId);
        }
      }
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to submit decision";
    }
  }

  // Auto-refresh data every 3 seconds
  $effect(() => {
    if (!projectId) return;
    fetchProjectData();
    const interval = setInterval(fetchProjectData, 3000);
    return () => clearInterval(interval);
  });

  // Auto-switch to discussion tab when in discussing state
  $effect(() => {
    if (project?.state === "discussing" && discussions.length > 0) {
      activeTab = "discussion";
    } else if (project?.state === "executing") {
      activeTab = "execution";
    }
  });

  function stateLabel(state: string): string {
    switch (state) {
      case "executing": return "Executing";
      case "verifying": return "Verifying";
      case "complete": return "Complete";
      case "blocked": return "Blocked";
      case "discussing": return "Discussing";
      case "planning": return "Planning";
      case "phase-planning": return "Planning phases";
      case "questioning": return "Questioning";
      case "researching": return "Researching";
      case "requirements": return "Requirements";
      case "roadmap": return "Roadmap";
      default: return state.charAt(0).toUpperCase() + state.slice(1);
    }
  }

  function isTabEnabled(tab: string): boolean {
    if (!project) return false;
    
    switch (tab) {
      case "overview": return true;
      case "requirements": return requirements.length > 0;
      case "roadmap": return phases.length > 0 || timeline !== null;
      case "execution": return project.state === "executing" || project.state === "verifying" || execution?.plans?.length > 0;
      case "discussion": return project.state === "discussing" && discussions.length > 0;
      default: return false;
    }
  }
</script>

<div class="planning-dashboard">
  <div class="dashboard-header">
    <div class="header-top">
      <div class="project-info">
        <h3 class="project-title">{project?.goal || "Loading..."}</h3>
        {#if project}
          <span class="project-state">{stateLabel(project.state)}</span>
        {/if}
      </div>
      <button class="close-btn" onclick={onClose} aria-label="Close dashboard">&times;</button>
    </div>
    
    <div class="tab-bar">
      <button 
        class="tab" 
        class:active={activeTab === "overview"}
        class:disabled={!isTabEnabled("overview")}
        onclick={() => { if (isTabEnabled("overview")) activeTab = "overview"; }}
      >
        Overview
      </button>
      <button 
        class="tab" 
        class:active={activeTab === "requirements"}
        class:disabled={!isTabEnabled("requirements")}
        onclick={() => { if (isTabEnabled("requirements")) activeTab = "requirements"; }}
      >
        Requirements {requirements.length > 0 ? `(${requirements.length})` : ""}
      </button>
      <button 
        class="tab" 
        class:active={activeTab === "roadmap"}
        class:disabled={!isTabEnabled("roadmap")}
        onclick={() => { if (isTabEnabled("roadmap")) activeTab = "roadmap"; }}
      >
        Roadmap {phases.length > 0 ? `(${phases.length})` : ""}
      </button>
      <button 
        class="tab" 
        class:active={activeTab === "execution"}
        class:disabled={!isTabEnabled("execution")}
        onclick={() => { if (isTabEnabled("execution")) activeTab = "execution"; }}
      >
        Execution
        {#if execution?.plans}
          {#if execution.plans.some(p => p.status === "running")}
            <Spinner size={12} />
          {:else}
            ({execution.plans.filter(p => p.status === "done").length}/{execution.plans.length})
          {/if}
        {/if}
      </button>
      <button 
        class="tab" 
        class:active={activeTab === "discussion"}
        class:disabled={!isTabEnabled("discussion")}
        onclick={() => { if (isTabEnabled("discussion")) activeTab = "discussion"; }}
      >
        Discussion {discussions.length > 0 ? `(${discussions.filter(q => q.status === "pending").length}/${discussions.length})` : ""}
      </button>
    </div>
  </div>

  <div class="dashboard-body">
    {#if loading}
      <div class="loading">
        <Spinner size={16} />
        <span>Loading project data...</span>
      </div>
    {:else if error}
      <div class="error">
        <span class="error-icon">⚠️</span>
        <span>{error}</span>
        <button onclick={() => fetchProjectData()}>Retry</button>
      </div>
    {:else}
      {#if activeTab === "overview"}
        <div class="overview-tab">
          {#if timeline}
            <RoadmapTimeline 
              milestones={timeline.milestones}
              currentState={project?.state || "idle"}
              onMilestoneClick={(id) => {
                if (id.startsWith("phase_")) {
                  activeTab = "roadmap";
                } else if (id === "requirements") {
                  activeTab = "requirements";
                }
              }}
            />
          {/if}
          
          {#if timeline?.requirementsSummary}
            <div class="summary-cards">
              <div class="summary-card">
                <div class="card-value">{timeline.requirementsSummary.v1}</div>
                <div class="card-label">v1 Requirements</div>
              </div>
              <div class="summary-card">
                <div class="card-value">{timeline.requirementsSummary.v2}</div>
                <div class="card-label">v2 Requirements</div>
              </div>
              <div class="summary-card">
                <div class="card-value">{timeline.requirementsSummary.outOfScope}</div>
                <div class="card-label">Out of Scope</div>
              </div>
            </div>
          {/if}
        </div>
      {:else if activeTab === "requirements"}
        <RequirementsTable {requirements} />
      {:else if activeTab === "roadmap"}
        <RoadmapTimeline 
          milestones={timeline?.milestones || []}
          currentState={project?.state || "idle"}
          detailed={true}
        />
      {:else if activeTab === "execution"}
        {#if execution}
          <ExecutionProgress {execution} />
        {:else}
          <div class="empty-state">
            <span>No execution data available</span>
          </div>
        {/if}
      {:else if activeTab === "discussion"}
        <div class="discussion-tab">
          {#if discussions.length > 0}
            {#each discussions as question (question.id)}
              <DiscussionCard 
                {question}
                onSubmit={(decision, userNote) => submitDiscussion(question.id, decision, userNote)}
              />
            {/each}
          {:else}
            <div class="empty-state">
              <span>No discussion questions available</span>
            </div>
          {/if}
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .planning-dashboard {
    width: 80vw;
    max-width: 1200px;
    height: 80vh;
    max-height: 800px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.2);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    z-index: 100;
  }

  .dashboard-header {
    background: var(--bg-elevated);
    border-bottom: 1px solid var(--border);
    padding: 16px 20px 0;
    flex-shrink: 0;
  }

  .header-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .project-info {
    flex: 1;
    min-width: 0;
  }

  .project-title {
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--text);
    margin: 0 0 4px 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .project-state {
    font-size: var(--text-sm);
    color: var(--text-secondary);
    background: var(--surface);
    padding: 2px 8px;
    border-radius: var(--radius-pill);
    border: 1px solid var(--border);
  }

  .close-btn {
    width: 32px;
    height: 32px;
    border: none;
    background: none;
    color: var(--text-muted);
    font-size: var(--text-xl);
    cursor: pointer;
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background var(--transition-quick), color var(--transition-quick);
  }

  .close-btn:hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  .tab-bar {
    display: flex;
    border-bottom: 1px solid var(--border);
    gap: 1px;
    margin: 0 -20px;
    padding: 0 20px;
  }

  .tab {
    padding: 12px 16px;
    border: none;
    background: none;
    color: var(--text-secondary);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    border-bottom: 2px solid transparent;
    transition: color var(--transition-quick), border-color var(--transition-quick);
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
  }

  .tab:hover:not(.disabled) {
    color: var(--text);
  }

  .tab.active {
    color: var(--accent);
    border-bottom-color: var(--accent);
  }

  .tab.disabled {
    color: var(--text-muted);
    cursor: not-allowed;
    opacity: 0.5;
  }

  .dashboard-body {
    flex: 1;
    overflow: auto;
    padding: 20px;
    min-height: 0;
  }

  .loading, .error, .empty-state {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 40px;
    text-align: center;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }

  .error {
    color: var(--status-error);
    flex-direction: column;
    gap: 16px;
  }

  .error-icon {
    font-size: var(--text-xl);
  }

  .error button {
    padding: 8px 16px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-size: var(--text-sm);
    transition: background var(--transition-quick);
  }

  .error button:hover {
    background: var(--surface-hover);
  }

  .overview-tab {
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .summary-cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 16px;
    margin-top: 16px;
  }

  .summary-card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: 16px;
    text-align: center;
  }

  .card-value {
    font-size: var(--text-2xl);
    font-weight: 700;
    color: var(--accent);
    margin-bottom: 4px;
  }

  .card-label {
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .discussion-tab {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  @media (max-width: 768px) {
    .planning-dashboard {
      width: 100vw;
      height: 100vh;
      max-width: 100vw;
      max-height: 100vh;
      border-radius: 0;
      top: 0;
      left: 0;
      transform: none;
    }

    .tab-bar {
      overflow-x: auto;
      scrollbar-width: none;
      -ms-overflow-style: none;
    }

    .tab-bar::-webkit-scrollbar {
      display: none;
    }

    .summary-cards {
      grid-template-columns: 1fr;
    }
  }
</style>