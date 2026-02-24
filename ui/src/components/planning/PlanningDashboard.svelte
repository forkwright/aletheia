<script lang="ts">
  import ProjectHeader from "./ProjectHeader.svelte";
  import RequirementsTable from "./RequirementsTable.svelte";
  import RoadmapView from "./RoadmapView.svelte";
  import ExecutionStatus from "./ExecutionStatus.svelte";
  import DiscussionPanel from "./DiscussionPanel.svelte";
  import ErrorBanner from "../shared/ErrorBanner.svelte";
  import Spinner from "../shared/Spinner.svelte";
  import { getActiveAgentId } from "../../stores/agents.svelte";

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
    projectContext?: any;
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
    order: number;
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
    if (!agentId) {
      loading = false;
      return;
    }

    try {
      loading = true;
      error = null;

      // First, get the active project for this agent
      const projectsRes = await fetch(`/api/planning/projects?nousId=${encodeURIComponent(agentId)}`);
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

      // Load full project data
      const projectId = activeProjects[0].id;
      const projectRes = await fetch(`/api/planning/projects/${projectId}`);
      if (!projectRes.ok) {
        throw new Error("Failed to load project details");
      }
      
      project = await projectRes.json() as Project;

      // Load requirements (from project context if available)
      requirements = extractRequirements(project);

      // Load roadmap (phases)
      try {
        const roadmapRes = await fetch(`/api/planning/projects/${projectId}/roadmap`);
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
        const executionRes = await fetch(`/api/planning/projects/${projectId}/execution`);
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

  function extractRequirements(proj: Project): Requirement[] {
    // Extract requirements from project context if available
    // This would be properly implemented based on the actual data structure
    if (proj.projectContext?.requirements) {
      return proj.projectContext.requirements.map((req: any, index: number) => ({
        id: req.id || `req-${index}`,
        name: req.name || `Requirement ${index + 1}`,
        description: req.description || "",
        tier: req.tier || "v1",
        rationale: req.rationale,
        category: req.category || "General"
      }));
    }
    return [];
  }

  // Load project when agent changes
  $effect(() => {
    if (agentId) {
      loadProject();
    }
  });

  // Auto-refresh every 30 seconds
  $effect(() => {
    if (!project) return;
    
    const interval = setInterval(loadProject, 30000);
    return () => clearInterval(interval);
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
    <ErrorBanner message={error} onClose={() => error = null} />
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
      />

      <!-- Main Dashboard Grid -->
      <div class="dashboard-grid">
        <!-- Requirements Section -->
        {#if requirements.length > 0}
          <div class="dashboard-section">
            <RequirementsTable {requirements} />
          </div>
        {/if}

        <!-- Roadmap Section -->
        {#if phases.length > 0}
          <div class="dashboard-section">
            <RoadmapView {phases} currentState={project.state} />
          </div>
        {/if}

        <!-- Execution Status -->
        {#if executionPlans.length > 0}
          <div class="dashboard-section">
            <ExecutionStatus plans={executionPlans} projectState={project.state} />
          </div>
        {/if}

        <!-- Discussion Panel (if in discussing state) -->
        {#if project.state === "discussing" && project.id}
          <div class="dashboard-section full-width">
            <DiscussionPanel projectId={project.id} />
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