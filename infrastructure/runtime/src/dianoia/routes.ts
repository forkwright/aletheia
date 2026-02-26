// Planning API routes — /api/planning/projects and /api/planning/projects/:id
import { Hono } from "hono";
import { createLogger } from "../koina/logger.js";
import { eventBus } from "../koina/event-bus.js";
import type { RouteDeps, RouteRefs } from "../pylon/routes/deps.js";
import { PlanningStore } from "./store.js";

const log = createLogger("pylon:planning");

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

  log.debug("planning routes mounted");
  return app;
}
