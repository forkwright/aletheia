<script lang="ts">
  import ProjectHeader from "./ProjectHeader.svelte";
  import RequirementsTable from "./RequirementsTable.svelte";
  import RoadmapView from "./RoadmapView.svelte";
  import ExecutionStatus from "./ExecutionStatus.svelte";
  import SpawnStatus from "./SpawnStatus.svelte";
  import MessageQueue from "./MessageQueue.svelte";
  import DiscussionPanel from "./DiscussionPanel.svelte";
  import VerificationPanel from "./VerificationPanel.svelte";
  import CheckpointApproval from "./CheckpointApproval.svelte";
  import RetrospectiveView from "./RetrospectiveView.svelte";
  import TimelineView from "./TimelineView.svelte";
  import TaskList from "./TaskList.svelte";
  import EditHistory from "./EditHistory.svelte";
  import ContextBudget from "./ContextBudget.svelte";
  import ErrorBanner from "../shared/ErrorBanner.svelte";
  import Spinner from "../shared/Spinner.svelte";
  import { getActiveAgentId } from "../../stores/agents.svelte";
  import { onGlobalEvent } from "../../lib/events.svelte";
  import { authFetch } from "./api";

  type PlanningLayout = "panel" | "half" | "full";

  // Props from parent component (ChatView)
  let { projectId: explicitProjectId, onClose, layout = "panel", onLayoutChange }: {
    projectId?: string;
    onClose?: () => void;
    layout?: PlanningLayout;
    onLayoutChange?: () => void;
  } = $props();

  interface Project {
    id: string;
    nousId: string;
    sessionId: string;
    goal: string;
    state: string;
    config: {
      name: string;
      description: string;
      scope?: string;
    };
    projectContext?: unknown;
    contextHash?: string;
    createdAt: string;
    updatedAt: string;
  }

  interface Requirement {
    id: string;
    name: string;
    description: string;
    tier: "v1" | "v2" | "out-of-scope";
    rationale?: string;
    category: string;
  }

  interface Phase {
    id: string;
    name: string;
    goal: string;
    dependencies: string[];
    requirements: string[];
    state: "pending" | "active" | "complete" | "blocked";
    status?: string;
    order: number;
    verificationResult?: unknown;
  }

  interface ExecutionPlan {
    phaseId: string;
    name: string;
    status: "pending" | "running" | "done" | "failed" | "skipped" | "zombie";
    waveNumber: number | null;
    startedAt: string | null;
    completedAt: string | null;
    error: string | null;
  }

  let project = $state<Project | null>(null);
  let requirements = $state<Requirement[]>([]);
  let phases = $state<Phase[]>([]);
  let executionPlans = $state<ExecutionPlan[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  const agentId = $derived(getActiveAgentId());

  async function loadProject() {
    if (!agentId && !explicitProjectId) {
      loading = false;
      return;
    }

    try {
      loading = true;
      error = null;

      let targetProjectId = explicitProjectId;
      
      if (!targetProjectId) {
        // First, get the active project for this agent
        const projectsRes = await authFetch(`/api/planning/projects?nousId=${encodeURIComponent(agentId!)}`);
        if (!projectsRes.ok) {
          throw new Error("Failed to load projects");
        }

        const projectsData = await projectsRes.json() as { projects: { id: string; state: string; goal: string }[] };
        const activeProjects = projectsData.projects?.filter(p => 
          p.state !== "complete" && p.state !== "abandoned"
        ) || [];

        if (activeProjects.length === 0) {
          project = null;
          loading = false;
          return;
        }

        targetProjectId = activeProjects[0].id;
      }

      // Load full project data
      const projectId = targetProjectId;
      const projectRes = await authFetch(`/api/planning/projects/${projectId}`);
      if (!projectRes.ok) {
        throw new Error("Failed to load project details");
      }
      
      project = await projectRes.json() as Project;

      // Load requirements from API
      requirements = await loadRequirements(targetProjectId!);

      // Load roadmap (phases)
      try {
        const roadmapRes = await authFetch(`/api/planning/projects/${projectId}/roadmap`);
        if (roadmapRes.ok) {
          const roadmapData = await roadmapRes.json();
          phases = roadmapData.phases || [];
        }
      } catch {
        // Roadmap might not exist yet
        phases = [];
      }

      // Load execution status
      try {
        const executionRes = await authFetch(`/api/planning/projects/${projectId}/execution`);
        if (executionRes.ok) {
          const executionData = await executionRes.json();
          executionPlans = executionData.plans || [];
        }
      } catch {
        // Execution might not be active yet
        executionPlans = [];
      }

    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  }

  async function loadRequirements(projectId: string): Promise<Requirement[]> {
    try {
      const res = await authFetch(`/api/planning/projects/${projectId}/requirements`);
      if (!res.ok) return [];
      const data = await res.json() as { requirements?: Array<{
        reqId: string; description: string; tier: string; rationale?: string; category: string;
      }> };
      return (data.requirements ?? []).map((req, index) => ({
        id: req.reqId || `req-${index}`,
        name: req.reqId || `Requirement ${index + 1}`,
        description: req.description || "",
        tier: (req.tier as "v1" | "v2" | "out-of-scope") || "v1",
        ...(req.rationale !== undefined && { rationale: req.rationale }),
        category: req.category || "General",
      }));
    } catch {
      return [];
    }
  }

  // Load project when agent or explicit project ID changes
  $effect(() => {
    if (agentId || explicitProjectId) {
      loadProject();
    }
  });

  // SSE-driven refresh: listen for planning events and reload
  $effect(() => {
    if (!project) return;
    
    const unsub = onGlobalEvent((event, _data) => {
      if (event.startsWith("planning:")) {
        loadProject();
      }
    });
    
    // Fallback polling: 10s during execution/verifying, 60s otherwise
    const isActive = project.state === "executing" || project.state === "verifying";
    const pollInterval = isActive ? 10000 : 60000;
    const interval = setInterval(loadProject, pollInterval);
    
    return () => { unsub(); clearInterval(interval); };
  });

  function stateLabel(state: string): string {
    const labels: Record<string, string> = {
      idle: "Not Started",
      questioning: "Gathering Context", 
      researching: "Research Phase",
      requirements: "Requirements Analysis",
      roadmap: "Roadmap Planning",
      "phase-planning": "Planning Phase",
      discussing: "Discussion Phase", 
      planning: "Planning",
      executing: "Executing",
      verifying: "Verification",
      complete: "Complete",
      blocked: "Blocked",
      abandoned: "Abandoned"
    };
    return labels[state] || state.charAt(0).toUpperCase() + state.slice(1);
  }

  function stateColor(state: string): string {
    if (state === "complete") return "var(--status-success)";
    if (state === "blocked" || state === "abandoned") return "var(--status-error)";
    if (state === "executing" || state === "verifying") return "var(--status-active)";
    if (state === "planning" || state === "discussing") return "var(--status-warning)";
    return "var(--text-muted)";
  }
</script>

<div class="planning-dashboard">
  {#if loading}
    <div class="loading-state">
      <Spinner size={20} />
      <span>Loading project...</span>
    </div>
  {:else if error}
    <ErrorBanner message={error} onDismiss={() => { error = null; }} />
  {:else if !project}
    <div class="empty-state">
      <div class="empty-icon">📋</div>
      <h2>No Active Planning Project</h2>
      <p>Start a new Dianoia project in chat to see the planning dashboard.</p>
    </div>
  {:else}
    <div class="dashboard-content">
      <!-- Project Header -->
      <ProjectHeader
        {project}
        stateLabel={stateLabel(project.state)}
        stateColor={stateColor(project.state)}
        onRefresh={loadProject}
        {layout}
        {...(onLayoutChange !== undefined && { onLayoutChange })}
        {...(onClose !== undefined && { onClose })}
      />

      <!-- Main Dashboard Grid -->
      <div class="dashboard-grid">
        <!-- Requirements Section -->
        {#if requirements.length > 0}
          <div class="dashboard-section">
            <RequirementsTable {requirements} projectId={project.id} />
          </div>
        {/if}

        <!-- Roadmap Section -->
        {#if phases.length > 0}
          <div class="dashboard-section">
            <RoadmapView {phases} currentState={project.state} projectId={project.id} />
          </div>
        {/if}

        <!-- Timeline (full width, always visible when phases exist) -->
        {#if phases.length > 0}
          <div class="dashboard-section full-width">
            <TimelineView projectId={project.id} />
          </div>
        {/if}

        <!-- Task List -->
        <div class="dashboard-section full-width">
          <TaskList projectId={project.id} />
        </div>

        <!-- Execution Status -->
        {#if executionPlans.length > 0}
          <div class="dashboard-section">
            <ExecutionStatus plans={executionPlans} projectState={project.state} />
          </div>
        {/if}

        <!-- Sub-Agent Status (INTERJ-04 / OBS-02) -->
        {#if ["executing", "verifying"].includes(project.state)}
          <div class="dashboard-section">
            <SpawnStatus projectId={project.id} />
          </div>
        {/if}

        <!-- Message Injection (INTERJ-01 / INTERJ-02) -->
        {#if ["executing", "verifying"].includes(project.state)}
          <div class="dashboard-section">
            <MessageQueue projectId={project.id} />
          </div>
        {/if}

        <!-- Verification (visible during verifying state or when any phase has results) -->
        {#if project.state === "verifying" || phases.some(p => p.verificationResult)}
          {@const verifyPhase = phases.find(p => p.verificationResult) ?? phases.find(p => p.status === "complete") ?? phases[0]}
          {#if verifyPhase}
            <div class="dashboard-section">
              <VerificationPanel projectId={project.id} phaseId={verifyPhase.id} phaseName={verifyPhase.name} />
            </div>
          {/if}
        {/if}

        <!-- Checkpoints (visible when project has any checkpoints or is blocked) -->
        {#if project.state === "blocked" || project.state === "executing" || project.state === "verifying"}
          <div class="dashboard-section full-width">
            <CheckpointApproval projectId={project.id} />
          </div>
        {/if}

        <!-- Discussion Panel (visible during discussing + phase-planning states) -->
        {#if (project.state === "discussing" || project.state === "phase-planning") && project.id}
          {@const activePhase = phases.find(p => p.state === "active" || p.status === "pending") ?? phases[0]}
          {#if activePhase}
            <div class="dashboard-section full-width">
              <DiscussionPanel projectId={project.id} phaseId={activePhase.id} />
            </div>
          {/if}
        {/if}

        <!-- Context Budget (OBS-04) — visible during execution -->
        {#if ["executing", "verifying", "phase-planning"].includes(project.state)}
          <div class="dashboard-section">
            <ContextBudget projectId={project.id} />
          </div>
        {/if}

        <!-- Edit History (SYNC-06) — always visible when project has content -->
        {#if requirements.length > 0 || phases.length > 0}
          <div class="dashboard-section full-width">
            <EditHistory projectId={project.id} />
          </div>
        {/if}

        <!-- Retrospective (visible when project is complete or abandoned) -->
        {#if project.state === "complete" || project.state === "abandoned"}
          <div class="dashboard-section full-width">
            <RetrospectiveView projectId={project.id} />
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .planning-dashboard {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    background: var(--bg);
  }

  .loading-state {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    height: 200px;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: var(--space-8);
    text-align: center;
  }

  .empty-icon {
    font-size: 4rem;
    margin-bottom: var(--space-4);
    opacity: 0.5;
  }

  .empty-state h2 {
    font-size: var(--text-xl);
    color: var(--text);
    margin-bottom: var(--space-2);
  }

  .empty-state p {
    color: var(--text-muted);
    font-size: var(--text-base);
    max-width: 400px;
    line-height: 1.5;
  }

  .dashboard-content {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .dashboard-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-4);
    padding: var(--space-4);
    overflow-y: auto;
    flex: 1;
  }

  .dashboard-section {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-4);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .dashboard-section.full-width {
    grid-column: 1 / -1;
  }

  @media (max-width: 1200px) {
    .dashboard-grid {
      grid-template-columns: 1fr;
      gap: var(--space-3);
      padding: var(--space-3);
    }

    .dashboard-section.full-width {
      grid-column: 1;
    }
  }

  @media (max-width: 768px) {
    .dashboard-grid {
      padding: var(--space-2);
      gap: var(--space-2);
    }

    .dashboard-section {
      padding: var(--space-3);
    }
  }
</style>