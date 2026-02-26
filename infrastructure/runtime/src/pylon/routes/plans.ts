// Plan routes — get, approve, cancel plans
import { Hono } from "hono";
import { createLogger } from "../../koina/logger.js";
import type { RouteDeps, RouteRefs } from "./deps.js";

const log = createLogger("pylon");

export function planRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const { manager, store } = deps;

  app.get("/api/plans/:id", (c) => {
    const plan = store.getPlan(c.req.param("id"));
    if (!plan) return c.json({ error: "Plan not found" }, 404);
    return c.json(plan);
  });

  app.get("/api/sessions/:id/plan", (c) => {
    const plan = store.getActivePlan(c.req.param("id"));
    if (!plan) return c.json({ error: "No active plan" }, 404);
    return c.json(plan);
  });

  app.post("/api/plans/:id/approve", async (c) => {
    const planId = c.req.param("id");
    const plan = store.getPlan(planId);
    if (!plan) return c.json({ error: "Plan not found" }, 404);
    if (plan.status !== "awaiting_approval") {
      return c.json({ error: `Plan is ${plan.status}, not awaiting_approval` }, 400);
    }

    let body: Record<string, unknown> = {};
    try { body = await c.req.json(); } catch { /* no body is fine — approve all */ }

    const skipSteps = (body["skip"] as number[] | undefined) ?? [];
    const steps = plan.steps.map((step) =>
      skipSteps.includes(step.id) ? { ...step, status: "skipped" as const } : { ...step, status: "approved" as const },
    );

    store.updatePlanSteps(planId, steps);
    store.updatePlanStatus(planId, "executing");

    const approvedCount = steps.filter(s => s.status === "approved").length;
    const skippedCount = steps.filter(s => s.status === "skipped").length;
    const summary = `Plan approved (${approvedCount} steps${skippedCount ? `, ${skippedCount} skipped` : ""}). Executing now.`;

    store.queueMessage(plan.sessionId, `[PLAN_APPROVED:${planId}] ${summary}`, "system");

    const session = store.findSessionById(plan.sessionId);
    const sessionKey = session?.sessionKey ?? "main";
    const lockKey = `${plan.nousId}:${sessionKey}`;

    if (!manager.isSessionActive(lockKey)) {
      setImmediate(() => {
        const gen = manager.handleMessageStreaming({
          text: `Execute the approved plan ${planId}. The plan steps are already stored — retrieve them and execute each approved step in order.`,
          nousId: plan.nousId,
          sessionKey,
        });
        (async () => { for await (const _ of gen) { /* drain */ } })().catch((error) => { log.warn(`Plan execution drain failed: ${error instanceof Error ? error.message : error}`); });
      });
    }

    return c.json({ ok: true, planId, approved: approvedCount, skipped: skippedCount });
  });

  app.post("/api/plans/:id/cancel", (c) => {
    const planId = c.req.param("id");
    const plan = store.getPlan(planId);
    if (!plan) return c.json({ error: "Plan not found" }, 404);
    if (plan.status !== "awaiting_approval" && plan.status !== "executing") {
      return c.json({ error: `Plan is ${plan.status}, cannot cancel` }, 400);
    }

    store.updatePlanStatus(planId, "cancelled");
    return c.json({ ok: true, planId, status: "cancelled" });
  });

  return app;
}
