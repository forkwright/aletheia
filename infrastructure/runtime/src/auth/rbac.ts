// Role-based access control
const ROUTE_PERMISSIONS: Record<string, string> = {
  // Chat
  "POST /api/sessions/send": "api:chat",
  "POST /api/sessions/stream": "api:chat",

  // Read
  "GET /api/sessions": "api:sessions:read",
  "GET /api/sessions/:id/history": "api:sessions:read",
  "GET /api/agents": "api:agents:read",
  "GET /api/agents/:id": "api:agents:read",
  "GET /api/agents/:id/identity": "api:agents:read",
  "GET /api/metrics": "api:metrics:read",
  "GET /api/costs/summary": "api:metrics:read",
  "GET /api/costs/session/:id": "api:metrics:read",
  "GET /api/costs/agent/:id": "api:metrics:read",
  "GET /api/events": "api:events",
  "GET /api/branding": "api:branding",
  "GET /api/skills": "api:agents:read",
  "GET /api/approval/mode": "api:agents:read",
  "GET /api/workspace/tree": "api:agents:read",
  "GET /api/workspace/file": "api:agents:read",
  "GET /api/workspace/git-status": "api:agents:read",

  // Admin
  "POST /api/sessions/:id/archive": "api:admin",
  "POST /api/sessions/:id/distill": "api:admin",
  "GET /api/cron": "api:admin",
  "POST /api/cron/:id/trigger": "api:admin",
  "GET /api/config": "api:admin",
  "GET /api/turns/active": "api:admin",
  "POST /api/turns/:id/abort": "api:admin",
  "POST /api/turns/:turnId/tools/:toolId/approve": "api:chat",
  "POST /api/turns/:turnId/tools/:toolId/deny": "api:chat",
  "GET /api/contacts/pending": "api:admin",
  "POST /api/contacts/:code/approve": "api:admin",
  "POST /api/contacts/:code/deny": "api:admin",
  "GET /api/export/stats": "api:admin",
  "GET /api/export/sessions": "api:admin",
  "GET /api/export/sessions/:id": "api:admin",
  "GET /api/blackboard": "api:admin",
  "POST /api/blackboard": "api:admin",
  "GET /api/memory/graph/export": "api:admin",
  "GET /api/memory/graph_stats": "api:admin",
  "POST /api/memory/graph/analyze": "api:admin",
  "GET /api/mcp/servers": "api:admin",
  "POST /api/mcp/servers/:name/reconnect": "api:admin",
  "GET /api/audit": "api:admin",
  "GET /api/auth/sessions": "api:admin",
  "POST /api/auth/revoke/:id": "api:admin",
};

const DEFAULT_ROLES: Record<string, string[]> = {
  admin: ["*"],
  user: [
    "api:chat",
    "api:sessions:read",
    "api:agents:read",
    "api:metrics:read",
    "api:events",
    "api:branding",
  ],
  readonly: [
    "api:sessions:read",
    "api:agents:read",
    "api:metrics:read",
    "api:branding",
  ],
};

function normalizeRoute(method: string, path: string): string {
  // Replace dynamic segments with :param placeholders for matching
  const normalized = path
    .replace(
      /\/api\/sessions\/[^/]+\/history/,
      "/api/sessions/:id/history",
    )
    .replace(
      /\/api\/sessions\/[^/]+\/archive/,
      "/api/sessions/:id/archive",
    )
    .replace(
      /\/api\/sessions\/[^/]+\/distill/,
      "/api/sessions/:id/distill",
    )
    .replace(/\/api\/agents\/[^/]+\/identity/, "/api/agents/:id/identity")
    .replace(/\/api\/agents\/[^/]+$/, "/api/agents/:id")
    .replace(
      /\/api\/turns\/[^/]+\/tools\/[^/]+\/approve/,
      "/api/turns/:turnId/tools/:toolId/approve",
    )
    .replace(
      /\/api\/turns\/[^/]+\/tools\/[^/]+\/deny/,
      "/api/turns/:turnId/tools/:toolId/deny",
    )
    .replace(/\/api\/turns\/[^/]+\/abort/, "/api/turns/:id/abort")
    .replace(
      /\/api\/contacts\/[^/]+\/approve/,
      "/api/contacts/:code/approve",
    )
    .replace(
      /\/api\/contacts\/[^/]+\/deny/,
      "/api/contacts/:code/deny",
    )
    .replace(
      /\/api\/costs\/session\/[^/]+/,
      "/api/costs/session/:id",
    )
    .replace(/\/api\/costs\/agent\/[^/]+/, "/api/costs/agent/:id")
    .replace(/\/api\/cron\/[^/]+\/trigger/, "/api/cron/:id/trigger")
    .replace(
      /\/api\/export\/sessions\/[^/]+$/,
      "/api/export/sessions/:id",
    )
    .replace(
      /\/api\/mcp\/servers\/[^/]+\/reconnect/,
      "/api/mcp/servers/:name/reconnect",
    )
    .replace(
      /\/api\/auth\/revoke\/[^/]+/,
      "/api/auth/revoke/:id",
    );

  return `${method} ${normalized}`;
}

export function getRequiredPermission(
  method: string,
  path: string,
): string | null {
  const key = normalizeRoute(method, path);
  return ROUTE_PERMISSIONS[key] ?? null;
}

export function hasPermission(
  role: string,
  permission: string,
  customRoles?: Record<string, string[]>,
): boolean {
  const roles = customRoles ?? DEFAULT_ROLES;
  const perms = roles[role];
  if (!perms) return false;
  if (perms.includes("*")) return true;
  return perms.includes(permission);
}
