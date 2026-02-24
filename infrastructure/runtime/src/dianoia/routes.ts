// Planning API routes — /api/planning/projects and /api/planning/projects/:id
import { Hono } from "hono";
import { createLogger } from "../koina/logger.js";
import type { RouteDeps, RouteRefs } from "../pylon/routes/deps.js";

const log = createLogger("pylon:planning");

export function planningRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const orch = deps.planningOrchestrator;

  app.get("/api/planning/projects", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    const projects = orch.listAllProjects();
    return c.json(
      projects.map((p) => ({
        id: p.id,
        goal: p.goal,
        state: p.state,
        createdAt: p.createdAt,
        updatedAt: p.updatedAt,
      })),
    );
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

  app.get("/api/planning/projects/:id/roadmap", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    const project = orch.getProject(c.req.param("id"));
    if (!project) return c.json({ error: "Project not found" }, 404);
    const phases = orch.listPhases(c.req.param("id"));
    return c.json({
      projectId: project.id,
      state: project.state,
      phases: phases.map((ph) => ({
        id: ph.id,
        name: ph.name,
        goal: ph.goal,
        requirements: ph.requirements,
        successCriteria: ph.successCriteria,
        phaseOrder: ph.phaseOrder,
        status: ph.status,
        hasPlan: ph.plan !== null,
      })),
    });
  });

  app.get("/api/planning/projects/:id/execution", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    const execOrch = deps.executionOrchestrator;
    if (!execOrch) return c.json({ error: "Execution orchestrator not available" }, 503);
    const project = orch.getProject(c.req.param("id"));
    if (!project) return c.json({ error: "Project not found" }, 404);
    const snapshot = execOrch.getExecutionSnapshot(project.id);
    return c.json(snapshot);
  });

  app.get("/api/planning/projects/:id/phases/:phaseId/status", (c) => {
    if (!orch) return c.json({ error: "Planning not enabled" }, 503);
    const execOrch = deps.executionOrchestrator;
    if (!execOrch) return c.json({ error: "Execution orchestrator not available" }, 503);
    const project = orch.getProject(c.req.param("id"));
    if (!project) return c.json({ error: "Project not found" }, 404);
    const phaseId = c.req.param("phaseId");
    const snapshot = execOrch.getExecutionSnapshot(project.id);
    const phasePlans = snapshot.plans.filter((p) => p.phaseId === phaseId);
    const phaseWaves = [...new Set(phasePlans.map((p) => p.waveNumber).filter((w): w is number => w !== null))];
    return c.json({
      phaseId,
      projectId: project.id,
      status: project.state,
      waveCount: phaseWaves.length > 0 ? Math.max(...phaseWaves) + 1 : 0,
      currentWave: snapshot.activeWave,
      plans: phasePlans,
    });
  });

  log.debug("planning routes mounted");
  return app;
}
