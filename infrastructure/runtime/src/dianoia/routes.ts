// Planning API routes — /api/planning/projects and /api/planning/projects/:id
//
// Phase 4 (ORCH-01 through ORCH-05, SYNC-01 through SYNC-04):
// - Full CRUD for requirements, phases, categories, discussions
// - Bidirectional file sync: every mutation writes SQLite AND markdown
// - SSE events emitted on every state change for real-time UI push
import { Hono } from "hono";
import { createLogger } from "../koina/logger.js";
import { eventBus } from "../koina/event-bus.js";
import type { RouteDeps, RouteRefs } from "../pylon/routes/deps.js";
import { PlanningStore } from "./store.js";
import { RequirementsOrchestrator } from "./requirements.js";
import type { CategoryProposal, ScopingDecision } from "./requirements.js";
import { writeRequirementsFile, writeRoadmapFile } from "./project-files.js";

const log = createLogger("pylon:planning");

/**
 * Sync requirements from SQLite to REQUIREMENTS.md.
 * Called after every requirement mutation so file stays co-primary with DB.
 */
function syncRequirementsFile(store: PlanningStore, projectId: string, workspaceRoot: string | null): void {
  if (!workspaceRoot) return;
  try {
    const reqs = store.listRequirements(projectId);
    writeRequirementsFile(workspaceRoot, projectId, reqs);
  } catch (err) {
    log.warn(`Failed to sync REQUIREMENTS.md for ${projectId}`, { error: err });
  }
}

/**
 * Sync phases from SQLite to ROADMAP.md.
 * Called after every phase mutation.
 */
function syncRoadmapFile(store: PlanningStore, projectId: string, workspaceRoot: string | null): void {
  if (!workspaceRoot) return;
  try {
    const phases = store.listPhases(projectId);
    writeRoadmapFile(workspaceRoot, projectId, phases);
  } catch (err) {
    log.warn(`Failed to sync ROADMAP.md for ${projectId}`, { error: err });
  }
}

export function planningRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const orch = deps.planningOrchestrator;

  // Get planning store for direct database access
  const getStore = (): PlanningStore | null => {
    try {
      return deps.store ? new PlanningStore(deps.store.getDb()) : null;
    } catch {
      return null;
    }
  };

  // Get workspace root from orchestrator for file sync
  const getWorkspaceRoot = (): string | null => {
    try {
      return orch?.getWorkspaceOrThrow() ?? null;
    } catch {
      return null;
    }
  };

  app.get("/api/planning/projects", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    
    const nousId = c.req.query("nousId");
    const projects = orch.listAllProjects();
    const filtered = nousId ? projects.filter(p => p.nousId === nousId) : projects;
    
    return c.json({
      projects: filtered.map((p) => ({
        id: p.id,
        goal: p.goal,
        state: p.state,
        activeWave: null, // Will be populated by execution endpoint
        createdAt: p.createdAt,
        updatedAt: p.updatedAt,
      }))
    });
  });

  app.get("/api/planning/projects/:id", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    const project = orch.getProject(c.req.param("id"));
    if (!project) return c.json({ error: "Project not found" }, 404);
    return c.json({
      id: project.id,
      nousId: project.nousId,
      sessionId: project.sessionId,
      goal: project.goal,
      state: project.state,
      config: project.config,
      projectContext: project.projectContext,
      contextHash: project.contextHash,
      createdAt: project.createdAt,
      updatedAt: project.updatedAt,
    });
  });

  // Execution status endpoint for real-time updates
  app.get("/api/planning/projects/:id/execution", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    
    const projectId = c.req.param("id");
    const project = orch.getProject(projectId);
    if (!project) return c.json({ error: "Project not found" }, 404);
    
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);
    
    const phases = orch.listPhases(projectId);
    const spawnRecords = store.listSpawnRecords(projectId);
    
    // Map phases to plan entries
    const plans = phases.map(phase => {
      const phaseRecords = spawnRecords.filter(r => r.phaseId === phase.id);
      const latestRecord = phaseRecords.toSorted((a, b) => 
        new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime()
      )[0];
      
      return {
        phaseId: phase.id,
        name: phase.name,
        status: latestRecord?.status || phase.status,
        waveNumber: latestRecord?.waveNumber || null,
        startedAt: latestRecord?.startedAt || null,
        completedAt: latestRecord?.completedAt || null,
        error: latestRecord?.errorMessage || null,
      };
    });
    
    // Find active plans and current wave
    const activePlans = plans.filter(p => p.status === "running").map(p => p.phaseId);
    const activeWaveNumbers = plans
      .filter(p => p.status === "running" && p.waveNumber !== null)
      .map(p => p.waveNumber as number);
    const activeWave = activeWaveNumbers.length > 0 ? Math.max(...activeWaveNumbers) : null;
    
    return c.json({
      projectId,
      state: project.state,
      activeWave,
      plans,
      activePlanIds: activePlans,
      startedAt: null, // TODO: Track project start time
      completedAt: null, // TODO: Track project completion time
    });
  });

  // Requirements endpoint for requirements table
  app.get("/api/planning/projects/:id/requirements", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    
    const projectId = c.req.param("id");
    const project = orch.getProject(projectId);
    if (!project) return c.json({ error: "Project not found" }, 404);
    
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);
    
    const requirements = store.listRequirements(projectId);
    
    return c.json({
      projectId,
      requirements: requirements.map(req => ({
        id: req.id,
        reqId: req.reqId,
        description: req.description,
        category: req.category,
        tier: req.tier,
        rationale: req.rationale,
        status: req.status,
        createdAt: req.createdAt,
        updatedAt: req.updatedAt,
      }))
    });
  });

  // Phases endpoint for roadmap visualization
  app.get("/api/planning/projects/:id/phases", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    
    const projectId = c.req.param("id");
    const project = orch.getProject(projectId);
    if (!project) return c.json({ error: "Project not found" }, 404);
    
    const phases = orch.listPhases(projectId);
    
    return c.json({
      projectId,
      phases: phases.map(phase => ({
        id: phase.id,
        name: phase.name,
        goal: phase.goal,
        status: phase.status,
        phaseOrder: phase.phaseOrder,
        requirements: phase.requirements,
        successCriteria: phase.successCriteria,
        verificationResult: phase.verificationResult,
        createdAt: phase.createdAt,
        updatedAt: phase.updatedAt,
      }))
    });
  });

  // Discussion endpoint for gray-area questions
  app.get("/api/planning/projects/:id/discuss", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    
    const projectId = c.req.param("id");
    const phaseId = c.req.query("phaseId");
    const project = orch.getProject(projectId);
    if (!project) return c.json({ error: "Project not found" }, 404);
    
    if (!phaseId) {
      return c.json({ error: "phaseId query parameter required" }, 400);
    }
    
    const discussions = orch.getPhaseDiscussions(projectId, phaseId);
    
    return c.json({
      projectId,
      phaseId,
      questions: discussions.map(q => ({
        id: q.id,
        question: q.question,
        options: q.options,
        recommendation: q.recommendation,
        decision: q.decision,
        userNote: q.userNote,
        status: q.status,
        createdAt: q.createdAt,
        updatedAt: q.updatedAt,
      }))
    });
  });

  // Submit discussion decision
  app.post("/api/planning/projects/:id/discuss", async (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    
    c.req.param("id"); // projectId - used for route matching
    const body = await c.req.json();
    const { questionId, decision, userNote } = body;
    
    if (!questionId || !decision) {
      return c.json({ error: "questionId and decision are required" }, 400);
    }
    
    try {
      orch.answerDiscussion(questionId, decision, userNote);
      return c.json({ success: true });
    } catch (error) {
      log.error("Failed to answer discussion", { questionId, error });
      return c.json({ error: "Failed to submit decision" }, 500);
    }
  });

  // Roadmap endpoint for legacy UI compatibility
  app.get("/api/planning/projects/:id/roadmap", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    
    const projectId = c.req.param("id");
    const project = orch.getProject(projectId);
    if (!project) return c.json({ error: "Project not found" }, 404);
    
    const phases = orch.listPhases(projectId);
    
    return c.json({
      projectId,
      phases: phases.map(phase => ({
        id: phase.id,
        name: phase.name,
        goal: phase.goal,
        dependencies: phase.dependencies ?? [],
        requirements: phase.requirements,
        state: phase.status,
        order: phase.phaseOrder,
        status: phase.status,
        phaseOrder: phase.phaseOrder,
        successCriteria: phase.successCriteria,
        verificationResult: phase.verificationResult,
        createdAt: phase.createdAt,
        updatedAt: phase.updatedAt,
      }))
    });
  });

  // Timeline endpoint for milestone visualization
  app.get("/api/planning/projects/:id/timeline", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    
    const timelineProjectId = c.req.param("id");
    const project = orch.getProject(timelineProjectId);
    if (!project) return c.json({ error: "Project not found" }, 404);
    
    const phases = orch.listPhases(timelineProjectId);
    const store = getStore();
    
    const requirements = store ? store.listRequirements(timelineProjectId) : [];
    
    // Create timeline milestones
    const milestones: Array<{
      id: string;
      name: string;
      type: "builtin" | "phase";
      status: "pending" | "active" | "complete" | "failed";
      order: number;
      goal?: string;
      requirements?: string[];
      requirementCount?: number;
    }> = [
      {
        id: "research",
        name: "Research",
        type: "builtin" as const,
        status: project.state === "researching" ? "active" : 
                ["requirements", "roadmap", "discussing", "planning", "executing", "verifying", "complete"].includes(project.state) ? "complete" : "pending",
        order: 0,
      },
      {
        id: "requirements",
        name: "Requirements",
        type: "builtin" as const,
        status: project.state === "requirements" ? "active" : 
                ["roadmap", "discussing", "planning", "executing", "verifying", "complete"].includes(project.state) ? "complete" : "pending",
        order: 1,
      }
    ];
    
    // Add phase milestones
    phases.forEach((phase, index) => {
      let status = "pending";
      if (phase.status === "executing") status = "active";
      else if (phase.status === "complete") status = "complete";
      else if (phase.status === "failed") status = "failed";
      
      milestones.push({
        id: phase.id,
        name: phase.name,
        type: "phase" as const,
        status: status as "pending" | "active" | "complete" | "failed",
        order: 2 + index,
        goal: phase.goal,
        requirements: phase.requirements,
        requirementCount: phase.requirements.length,
      });
    });
    
    return c.json({
      projectId: timelineProjectId,
      goal: project.goal,
      state: project.state,
      milestones: milestones.toSorted((a, b) => a.order - b.order),
      requirementsSummary: {
        v1: requirements.filter(r => r.tier === "v1").length,
        v2: requirements.filter(r => r.tier === "v2").length,
        outOfScope: requirements.filter(r => r.tier === "out-of-scope").length,
      }
    });
  });

  // Checkpoints endpoint — list and manage human-in-loop gates
  app.get("/api/planning/projects/:id/checkpoints", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    
    const projectId = c.req.param("id");
    const project = orch.getProject(projectId);
    if (!project) return c.json({ error: "Project not found" }, 404);
    
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);
    
    const checkpoints = store.listCheckpoints(projectId);
    
    return c.json({
      projectId,
      checkpoints: checkpoints.map(cp => ({
        id: cp.id,
        type: cp.type,
        question: cp.question,
        decision: cp.decision,
        context: cp.context,
        createdAt: cp.createdAt,
      }))
    });
  });

  // Approve or skip a checkpoint
  app.post("/api/planning/projects/:id/checkpoints/:checkpointId", async (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    
    const projectId = c.req.param("id");
    const checkpointId = c.req.param("checkpointId");
    const project = orch.getProject(projectId);
    if (!project) return c.json({ error: "Project not found" }, 404);
    
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);
    
    const body = await c.req.json();
    const { action, note } = body as { action: string; note?: string };
    
    if (action !== "approve" && action !== "skip") {
      return c.json({ error: "action must be 'approve' or 'skip'" }, 400);
    }
    
    try {
      store.resolveCheckpoint(checkpointId, action === "approve" ? "approved" : "skipped", 
        note ? { userNote: note } : undefined);
      return c.json({ success: true, decision: action });
    } catch (error) {
      log.error("Failed to resolve checkpoint", { checkpointId, error });
      return c.json({ error: "Failed to resolve checkpoint" }, 500);
    }
  });

  // Verification results for a specific phase
  app.get("/api/planning/projects/:id/phases/:phaseId/verification", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    
    const projectId = c.req.param("id");
    const phaseId = c.req.param("phaseId");
    const project = orch.getProject(projectId);
    if (!project) return c.json({ error: "Project not found" }, 404);
    
    const phases = orch.listPhases(projectId);
    const phase = phases.find(p => p.id === phaseId);
    if (!phase) return c.json({ error: "Phase not found" }, 404);
    
    const verification = phase.verificationResult;
    if (!verification) {
      return c.json({ projectId, phaseId, verification: null });
    }
    
    return c.json({
      projectId,
      phaseId,
      verification: {
        status: verification.status ?? verification.overallStatus ?? "unknown",
        summary: verification.summary,
        gaps: verification.gaps ?? [],
        verifiedAt: verification.verifiedAt,
        overridden: verification.overridden ?? false,
        overrideNote: verification.overrideNote,
      }
    });
  });

  // Retrospective for a completed/abandoned project
  app.get("/api/planning/projects/:id/retrospective", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    
    const projectId = c.req.param("id");
    const project = orch.getProject(projectId);
    if (!project) return c.json({ error: "Project not found" }, 404);
    
    if (project.state !== "complete" && project.state !== "abandoned") {
      return c.json({ 
        projectId,
        retrospective: null,
        reason: "Project is in '" + project.state + "' state — retrospective available after completion or abandonment"
      });
    }
    
    try {
      const retro = orch.generateRetrospective(projectId);
      return c.json({
        projectId,
        retrospective: {
          goal: retro.goal,
          outcome: retro.outcome,
          phases: retro.phases,
          patterns: retro.patterns,
          generatedAt: retro.generatedAt,
        }
      });
    } catch (error) {
      log.error("Failed to generate retrospective", { projectId, error });
      return c.json({ error: "Failed to generate retrospective" }, 500);
    }
  });

  // ============================================================
  // CRUD — Requirements (ORCH-01)
  // ============================================================

  // Create a new requirement
  app.post("/api/planning/projects/:id/requirements", async (c) => {
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);

    const projectId = c.req.param("id");
    const body = await c.req.json() as {
      description: string;
      category: string;
      tier?: "v1" | "v2" | "out-of-scope";
      rationale?: string;
      reqId?: string;
      phaseId?: string;
    };

    if (!body.description || !body.category) {
      return c.json({ error: "description and category are required" }, 400);
    }

    try {
      const reqId = body.reqId ?? store.nextReqId(projectId, body.category);
      const req = store.createRequirement({
        projectId,
        reqId,
        description: body.description,
        category: body.category,
        tier: body.tier ?? "v1",
        rationale: body.rationale ?? null,
        phaseId: body.phaseId ?? null,
      });
      syncRequirementsFile(store, projectId, getWorkspaceRoot());
      eventBus.emit("planning:requirement-changed", {
        projectId, action: "created", requirementId: req.id, reqId: req.reqId,
      });
      return c.json(req, 201);
    } catch (error) {
      log.error("Failed to create requirement", { projectId, error });
      return c.json({ error: "Failed to create requirement" }, 500);
    }
  });

  // Update a requirement (by internal ID or reqId)
  app.patch("/api/planning/projects/:id/requirements/:reqIdentifier", async (c) => {
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);

    const projectId = c.req.param("id");
    const reqIdentifier = c.req.param("reqIdentifier");
    const body = await c.req.json() as {
      tier?: "v1" | "v2" | "out-of-scope";
      rationale?: string | null;
      description?: string;
      category?: string;
      reqId?: string;
      status?: "pending" | "validated" | "skipped";
      phaseId?: string | null;
    };

    try {
      // Resolve identifier: try as internal ID first, then as reqId
      let req = store.getRequirement(reqIdentifier);
      if (!req) {
        req = store.getRequirementByReqId(projectId, reqIdentifier);
      }
      if (!req) {
        return c.json({ error: "Requirement not found" }, 404);
      }

      const updated = store.updateRequirement(req.id, body);
      syncRequirementsFile(store, projectId, getWorkspaceRoot());
      eventBus.emit("planning:requirement-changed", {
        projectId, action: "updated", requirementId: req.id, reqId: updated.reqId, changes: Object.keys(body),
      });
      return c.json(updated);
    } catch (error) {
      log.error("Failed to update requirement", { projectId, reqIdentifier, error });
      return c.json({ error: "Failed to update requirement" }, 500);
    }
  });

  // Delete a requirement
  app.delete("/api/planning/projects/:id/requirements/:reqIdentifier", (c) => {
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);

    const projectId = c.req.param("id");
    const reqIdentifier = c.req.param("reqIdentifier");

    try {
      let req = store.getRequirement(reqIdentifier);
      if (!req) {
        req = store.getRequirementByReqId(projectId, reqIdentifier);
      }
      if (!req) {
        return c.json({ error: "Requirement not found" }, 404);
      }

      store.deleteRequirement(req.id);
      syncRequirementsFile(store, projectId, getWorkspaceRoot());
      eventBus.emit("planning:requirement-changed", {
        projectId, action: "deleted", requirementId: req.id, reqId: req.reqId,
      });
      return c.json({ success: true, deleted: req.reqId });
    } catch (error) {
      log.error("Failed to delete requirement", { projectId, reqIdentifier, error });
      return c.json({ error: "Failed to delete requirement" }, 500);
    }
  });

  // ============================================================
  // CRUD — Phases (ORCH-02)
  // ============================================================

  // Update phase metadata
  app.patch("/api/planning/projects/:id/phases/:phaseId", async (c) => {
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);

    const projectId = c.req.param("id");
    const phaseId = c.req.param("phaseId");
    const body = await c.req.json() as {
      name?: string;
      goal?: string;
      successCriteria?: string[];
      requirements?: string[];
    };

    try {
      const phase = store.getPhase(phaseId);
      if (!phase || phase.projectId !== projectId) {
        return c.json({ error: "Phase not found" }, 404);
      }

      const updated = store.updatePhase(phaseId, body);
      syncRoadmapFile(store, projectId, getWorkspaceRoot());
      eventBus.emit("planning:phase-changed", {
        projectId, action: "updated", phaseId, changes: Object.keys(body),
      });
      return c.json({
        id: updated.id,
        name: updated.name,
        goal: updated.goal,
        status: updated.status,
        phaseOrder: updated.phaseOrder,
        requirements: updated.requirements,
        successCriteria: updated.successCriteria,
        dependencies: updated.dependencies,
        createdAt: updated.createdAt,
        updatedAt: updated.updatedAt,
      });
    } catch (error) {
      log.error("Failed to update phase", { projectId, phaseId, error });
      return c.json({ error: "Failed to update phase" }, 500);
    }
  });

  // Delete a phase
  app.delete("/api/planning/projects/:id/phases/:phaseId", (c) => {
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);

    const projectId = c.req.param("id");
    const phaseId = c.req.param("phaseId");

    try {
      const phase = store.getPhase(phaseId);
      if (!phase || phase.projectId !== projectId) {
        return c.json({ error: "Phase not found" }, 404);
      }

      store.deletePhase(phaseId);
      syncRoadmapFile(store, projectId, getWorkspaceRoot());
      eventBus.emit("planning:phase-changed", {
        projectId, action: "deleted", phaseId, phaseName: phase.name,
      });
      return c.json({ success: true, deleted: phase.name });
    } catch (error) {
      log.error("Failed to delete phase", { projectId, phaseId, error });
      return c.json({ error: "Failed to delete phase" }, 500);
    }
  });

  // Reorder a phase
  app.post("/api/planning/projects/:id/phases/:phaseId/reorder", async (c) => {
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);

    const projectId = c.req.param("id");
    const phaseId = c.req.param("phaseId");
    const body = await c.req.json() as { newOrder: number };

    if (typeof body.newOrder !== "number") {
      return c.json({ error: "newOrder (number) is required" }, 400);
    }

    try {
      store.reorderPhase(projectId, phaseId, body.newOrder);
      const phases = store.listPhases(projectId);
      syncRoadmapFile(store, projectId, getWorkspaceRoot());
      eventBus.emit("planning:phase-changed", {
        projectId, action: "reordered", phaseId, newOrder: body.newOrder,
      });
      return c.json({
        projectId,
        phases: phases.map(p => ({
          id: p.id,
          name: p.name,
          phaseOrder: p.phaseOrder,
          status: p.status,
        })),
      });
    } catch (error) {
      log.error("Failed to reorder phase", { projectId, phaseId, error });
      return c.json({ error: "Failed to reorder phase" }, 500);
    }
  });

  // ============================================================
  // Discussion answer with SSE event (extends existing POST)
  // ============================================================

  // The existing POST /api/planning/projects/:id/discuss handler above
  // already calls orch.answerDiscussion(). We just need the SSE event emitted.
  // Rather than duplicate the route, patch the existing handler's success path.
  // NOTE: Since the existing handler is already registered above, we add a
  // middleware-style wrapper. But Hono doesn't support post-handler hooks easily,
  // so instead we emit from a separate dedicated endpoint for UI-driven answers.

  app.post("/api/planning/projects/:id/discuss/answer", async (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);

    const projectId = c.req.param("id");
    const body = await c.req.json() as {
      questionId: string;
      decision: string;
      userNote?: string;
    };

    if (!body.questionId || !body.decision) {
      return c.json({ error: "questionId and decision are required" }, 400);
    }

    try {
      orch.answerDiscussion(body.questionId, body.decision, body.userNote);
      eventBus.emit("planning:discussion-answered", {
        projectId, questionId: body.questionId, decision: body.decision,
      });
      return c.json({ success: true });
    } catch (error) {
      log.error("Failed to answer discussion", { projectId, error });
      return c.json({ error: "Failed to submit decision" }, 500);
    }
  });

  // Skip a discussion question
  app.post("/api/planning/projects/:id/discuss/skip", async (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);

    const projectId = c.req.param("id");
    const body = await c.req.json() as { questionId: string };

    if (!body.questionId) {
      return c.json({ error: "questionId is required" }, 400);
    }

    try {
      const store = getStore();
      if (!store) return c.json({ error: "Database not available" }, 503);
      // Skip = answer with recommendation or mark as skipped
      store.answerDiscussionQuestion(body.questionId, "[skipped — agent uses recommendation]");
      eventBus.emit("planning:discussion-answered", {
        projectId, questionId: body.questionId, action: "skipped",
      });
      return c.json({ success: true });
    } catch (error) {
      log.error("Failed to skip discussion", { projectId, error });
      return c.json({ error: "Failed to skip discussion" }, 500);
    }
  });

  // ============================================================
  // Category Proposal Workflow (ORCH-03)
  // ============================================================

  // Present a category proposal — returns formatted text for UI display
  app.post("/api/planning/projects/:id/categories/present", async (c) => {
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);

    const projectId = c.req.param("id");
    const body = await c.req.json() as CategoryProposal;

    if (!body.category || !body.categoryName) {
      return c.json({ error: "category and categoryName are required" }, 400);
    }

    try {
      const reqOrch = new RequirementsOrchestrator(store["db"], getWorkspaceRoot() ?? undefined);
      const formatted = reqOrch.formatCategoryPresentation(body);
      eventBus.emit("planning:requirement-changed", {
        projectId, action: "category-presented", category: body.category,
      });
      return c.json({
        projectId,
        category: body.category,
        categoryName: body.categoryName,
        formatted,
        tableStakesCount: body.tableStakes.length,
        differentiatorCount: body.differentiators.length,
      });
    } catch (error) {
      log.error("Failed to present category", { projectId, error });
      return c.json({ error: "Failed to present category" }, 500);
    }
  });

  // Persist category decisions — creates requirements from approved features
  app.post("/api/planning/projects/:id/categories/persist", async (c) => {
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);

    const projectId = c.req.param("id");
    const body = await c.req.json() as {
      category: CategoryProposal;
      decisions: ScopingDecision[];
    };

    if (!body.category || !body.decisions?.length) {
      return c.json({ error: "category and decisions[] are required" }, 400);
    }

    try {
      const reqOrch = new RequirementsOrchestrator(store["db"], getWorkspaceRoot() ?? undefined);
      reqOrch.persistCategory(projectId, body.category, body.decisions);
      // File sync is already handled by RequirementsOrchestrator.persistCategory()
      // but let's ensure it happens even if workspace was null during construction
      syncRequirementsFile(store, projectId, getWorkspaceRoot());
      eventBus.emit("planning:requirement-changed", {
        projectId, action: "category-persisted", category: body.category.category,
        count: body.decisions.length,
      });
      const reqs = store.listRequirements(projectId);
      return c.json({
        projectId,
        category: body.category.category,
        persisted: body.decisions.length,
        totalRequirements: reqs.length,
      }, 201);
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      const code = (error as any)?.code ?? "";
      log.error("Failed to persist category", { projectId, error });
      if (msg.includes("Table-stakes") || msg.includes("TABLE_STAKES") || code.includes("TABLE_STAKES") ||
          msg.includes("DUPLICATE") || code.includes("DUPLICATE")) {
        return c.json({ error: msg }, 400);
      }
      return c.json({ error: "Failed to persist category" }, 500);
    }
  });

  // Adjust a category's requirements — bulk update tiers/rationale
  app.patch("/api/planning/projects/:id/categories/:categoryCode", async (c) => {
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);

    const projectId = c.req.param("id");
    const categoryCode = c.req.param("categoryCode");
    const body = await c.req.json() as {
      adjustments: Array<{
        reqId: string;
        tier?: "v1" | "v2" | "out-of-scope";
        rationale?: string | null;
      }>;
    };

    if (!body.adjustments?.length) {
      return c.json({ error: "adjustments[] is required" }, 400);
    }

    try {
      const results: Array<{ reqId: string; updated: boolean; error?: string }> = [];
      for (const adj of body.adjustments) {
        try {
          const req = store.getRequirementByReqId(projectId, adj.reqId);
          if (!req) {
            results.push({ reqId: adj.reqId, updated: false, error: "not found" });
            continue;
          }
          if (req.category !== categoryCode) {
            results.push({ reqId: adj.reqId, updated: false, error: `belongs to ${req.category}, not ${categoryCode}` });
            continue;
          }
          const updates: Record<string, unknown> = {};
          if (adj.tier !== undefined) updates["tier"] = adj.tier;
          if (adj.rationale !== undefined) updates["rationale"] = adj.rationale;
          store.updateRequirement(req.id, updates as { tier?: "v1" | "v2" | "out-of-scope"; rationale?: string | null });
          results.push({ reqId: adj.reqId, updated: true });
        } catch (err) {
          results.push({ reqId: adj.reqId, updated: false, error: err instanceof Error ? err.message : String(err) });
        }
      }

      syncRequirementsFile(store, projectId, getWorkspaceRoot());
      const updatedCount = results.filter(r => r.updated).length;
      eventBus.emit("planning:requirement-changed", {
        projectId, action: "category-adjusted", category: categoryCode, updatedCount,
      });
      return c.json({ projectId, category: categoryCode, results, updatedCount });
    } catch (error) {
      log.error("Failed to adjust category", { projectId, categoryCode, error });
      return c.json({ error: "Failed to adjust category" }, 500);
    }
  });

  // ============================================================
  // Batch Operations (ORCH-06) — v2 but useful now
  // ============================================================

  app.post("/api/planning/projects/:id/batch", async (c) => {
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);

    const projectId = c.req.param("id");
    const project = orch?.getProject(projectId);
    if (!project) return c.json({ error: "Project not found" }, 404);

    const body = await c.req.json() as {
      operations: Array<{
        type: "update-requirement" | "update-phase" | "delete-requirement" | "delete-phase";
        id: string;
        data?: Record<string, unknown>;
      }>;
    };

    if (!body.operations?.length) {
      return c.json({ error: "operations[] is required" }, 400);
    }

    if (body.operations.length > 50) {
      return c.json({ error: "Maximum 50 operations per batch" }, 400);
    }

    const results: Array<{ index: number; type: string; id: string; success: boolean; error?: string }> = [];
    let reqsChanged = false;
    let phasesChanged = false;

    for (let i = 0; i < body.operations.length; i++) {
      const op = body.operations[i]!;
      try {
        switch (op.type) {
          case "update-requirement": {
            const req = store.getRequirement(op.id) ?? store.getRequirementByReqId(projectId, op.id);
            if (!req) throw new Error("Not found");
            store.updateRequirement(req.id, op.data as { tier?: "v1" | "v2" | "out-of-scope"; rationale?: string | null });
            reqsChanged = true;
            results.push({ index: i, type: op.type, id: op.id, success: true });
            break;
          }
          case "delete-requirement": {
            const req = store.getRequirement(op.id) ?? store.getRequirementByReqId(projectId, op.id);
            if (!req) throw new Error("Not found");
            store.deleteRequirement(req.id);
            reqsChanged = true;
            results.push({ index: i, type: op.type, id: op.id, success: true });
            break;
          }
          case "update-phase": {
            store.updatePhase(op.id, op.data as { name?: string; goal?: string });
            phasesChanged = true;
            results.push({ index: i, type: op.type, id: op.id, success: true });
            break;
          }
          case "delete-phase": {
            store.deletePhase(op.id);
            phasesChanged = true;
            results.push({ index: i, type: op.type, id: op.id, success: true });
            break;
          }
          default:
            results.push({ index: i, type: op.type, id: op.id, success: false, error: `Unknown type: ${op.type}` });
        }
      } catch (err) {
        results.push({ index: i, type: op.type, id: op.id, success: false, error: err instanceof Error ? err.message : String(err) });
      }
    }

    const wsRoot = getWorkspaceRoot();
    if (reqsChanged) syncRequirementsFile(store, projectId, wsRoot);
    if (phasesChanged) syncRoadmapFile(store, projectId, wsRoot);

    const successCount = results.filter(r => r.success).length;
    eventBus.emit("planning:phase-changed", {
      projectId, action: "batch", operationCount: body.operations.length, successCount,
    });

    return c.json({
      projectId,
      totalOperations: body.operations.length,
      successCount,
      failureCount: body.operations.length - successCount,
      results,
    });
  });

  // ============================================================
  // Decision Audit Trail (OBS-03)
  // ============================================================

  app.get("/api/planning/projects/:id/decisions", (c) => {
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);

    const projectId = c.req.param("id");
    const phaseId = c.req.query("phaseId");

    try {
      const decisions = store.listDecisions(projectId, phaseId || undefined);
      return c.json({ projectId, decisions, count: decisions.length });
    } catch (error) {
      log.error("Failed to list decisions", { projectId, error });
      return c.json({ error: "Failed to list decisions" }, 500);
    }
  });

  // ============================================================
  // Turn Tracking (OBS-05)
  // ============================================================

  app.get("/api/planning/projects/:id/usage", (c) => {
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);

    const projectId = c.req.param("id");
    const phaseId = c.req.query("phaseId");

    try {
      const turnCounts = store.getTurnCounts(projectId, phaseId || undefined);
      const totals = store.getProjectTurnTotal(projectId);
      return c.json({
        projectId,
        turnCounts,
        totals,
      });
    } catch (error) {
      log.error("Failed to get usage", { projectId, error });
      return c.json({ error: "Failed to get usage" }, 500);
    }
  });

  // ============================================================
  // Spawn Records (INTERJ-04 / OBS-02)
  // ============================================================

  app.get("/api/planning/projects/:id/spawns", (c) => {
    const store = getStore();
    if (!store) return c.json({ error: "Database not available" }, 503);

    const projectId = c.req.param("id");
    const phaseId = c.req.query("phaseId");
    const status = c.req.query("status");

    try {
      let records = store.listSpawnRecords(projectId);

      if (phaseId) {
        records = records.filter((r) => r.phaseId === phaseId);
      }
      if (status) {
        records = records.filter((r) => r.status === status);
      }

      const summary = {
        total: records.length,
        running: records.filter((r) => r.status === "running").length,
        complete: records.filter((r) => r.status === "complete" || r.status === "done").length,
        failed: records.filter((r) => r.status === "failed").length,
        pending: records.filter((r) => r.status === "pending").length,
      };

      return c.json({ projectId, spawns: records, summary });
    } catch (error) {
      log.error("Failed to list spawn records", { projectId, error });
      return c.json({ error: "Failed to list spawn records" }, 500);
    }
  });

  // ─── Message Injection (INTERJ-01/02) ────────────────────

  /**
   * POST /api/planning/projects/:id/messages — Inject a message into a running execution
   *
   * Body: { content: string, source?: "user"|"agent"|"sub-agent"|"system", phaseId?: string,
   *         priority?: "low"|"normal"|"high"|"critical", sourceSessionId?: string, expiresInSeconds?: number }
   *
   * The message is queued and consumed at the next turn boundary (between waves or between tasks).
   * Critical-priority messages pause execution immediately.
   */
  app.post("/api/planning/projects/:id/messages", async (c) => {
    const projectId = c.req.param("id");
    const store = getStore();
    if (!store) return c.json({ error: "Store not available" }, 503);

    try {
      const body = await c.req.json() as Record<string, unknown>;
      const content = body["content"] as string;
      if (!content || typeof content !== "string" || content.trim().length === 0) {
        return c.json({ error: "content is required and must be a non-empty string" }, 400);
      }

      const source = (body["source"] as string | undefined) ?? "user";
      const validSources = ["user", "agent", "sub-agent", "system"];
      if (!validSources.includes(source)) {
        return c.json({ error: `source must be one of: ${validSources.join(", ")}` }, 400);
      }

      const priority = (body["priority"] as string | undefined) ?? "normal";
      const validPriorities = ["low", "normal", "high", "critical"];
      if (!validPriorities.includes(priority)) {
        return c.json({ error: `priority must be one of: ${validPriorities.join(", ")}` }, 400);
      }

      const expiresInSeconds = body["expiresInSeconds"] as number | undefined;
      let expiresAt: string | undefined;
      if (expiresInSeconds && expiresInSeconds > 0) {
        expiresAt = new Date(Date.now() + expiresInSeconds * 1000).toISOString();
      }

      const enqueueOpts: Parameters<typeof store.enqueueMessage>[0] = {
        projectId,
        source: source as "user" | "agent" | "sub-agent" | "system",
        content: content.trim(),
        priority: priority as "low" | "normal" | "high" | "critical",
      };
      const phaseIdVal = body["phaseId"] as string | undefined;
      if (phaseIdVal) enqueueOpts.phaseId = phaseIdVal;
      const sessionIdVal = body["sourceSessionId"] as string | undefined;
      if (sessionIdVal) enqueueOpts.sourceSessionId = sessionIdVal;
      if (expiresAt) enqueueOpts.expiresAt = expiresAt;

      const message = store.enqueueMessage(enqueueOpts);

      eventBus.emit("planning:message-enqueued", { projectId, messageId: message.id, priority, source });
      log.info(`Message ${message.id} enqueued for project ${projectId}: [${priority}] ${content.slice(0, 80)}`);

      return c.json({ message }, 201);
    } catch (error) {
      log.error("Failed to enqueue message", { projectId, error });
      return c.json({ error: "Failed to enqueue message" }, 500);
    }
  });

  /**
   * GET /api/planning/projects/:id/messages — List messages for a project
   *
   * Query params: ?status=pending|delivered|expired, ?phaseId=xxx
   */
  app.get("/api/planning/projects/:id/messages", (c) => {
    const projectId = c.req.param("id");
    const store = getStore();
    if (!store) return c.json({ error: "Store not available" }, 503);

    try {
      const listOpts: Parameters<typeof store.listMessages>[1] = {};
      const statusParam = c.req.query("status") as "pending" | "delivered" | "expired" | undefined;
      if (statusParam) listOpts.status = statusParam;
      const phaseIdParam = c.req.query("phaseId");
      if (phaseIdParam) listOpts.phaseId = phaseIdParam;
      const messages = store.listMessages(projectId, listOpts);
      const pendingCount = store.countPendingMessages(projectId);

      return c.json({ projectId, messages, pendingCount });
    } catch (error) {
      log.error("Failed to list messages", { projectId, error });
      return c.json({ error: "Failed to list messages" }, 500);
    }
  });

  log.debug("planning routes mounted");
  return app;
}
