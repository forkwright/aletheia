// RBAC tests
import { describe, expect, it } from "vitest";
import { getRequiredPermission, hasPermission } from "./rbac.js";

describe("getRequiredPermission", () => {
  it("returns permission for known route", () => {
    expect(getRequiredPermission("POST", "/api/sessions/send")).toBe("api:chat");
  });

  it("returns null for unknown route", () => {
    expect(getRequiredPermission("GET", "/api/nonexistent")).toBeNull();
  });

  it("normalizes session ID in path", () => {
    expect(getRequiredPermission("GET", "/api/sessions/abc-123-def/history")).toBe("api:sessions:read");
  });

  it("normalizes agent ID in path", () => {
    expect(getRequiredPermission("GET", "/api/agents/main")).toBe("api:agents:read");
  });

  it("normalizes nested tool approval path", () => {
    expect(getRequiredPermission("POST", "/api/turns/t_abc/tools/tool_123/approve")).toBe("api:chat");
  });

  it("normalizes cost session path", () => {
    expect(getRequiredPermission("GET", "/api/costs/session/ses_abc")).toBe("api:metrics:read");
  });

  it("normalizes cost agent path", () => {
    expect(getRequiredPermission("GET", "/api/costs/agent/main")).toBe("api:metrics:read");
  });

  it("normalizes cron trigger path", () => {
    expect(getRequiredPermission("POST", "/api/cron/heartbeat/trigger")).toBe("api:admin");
  });

  it("normalizes contacts approve path", () => {
    expect(getRequiredPermission("POST", "/api/contacts/CODE123/approve")).toBe("api:admin");
  });

  it("normalizes reflection path", () => {
    expect(getRequiredPermission("GET", "/api/reflection/main")).toBe("api:admin");
  });

  it("normalizes reflection assessment path", () => {
    expect(getRequiredPermission("GET", "/api/reflection/main/assessment")).toBe("api:admin");
  });

  it("normalizes export sessions path", () => {
    expect(getRequiredPermission("GET", "/api/export/sessions/ses_abc")).toBe("api:admin");
  });

  it("normalizes mcp reconnect path", () => {
    expect(getRequiredPermission("POST", "/api/mcp/servers/myserver/reconnect")).toBe("api:admin");
  });

  it("normalizes auth revoke path", () => {
    expect(getRequiredPermission("POST", "/api/auth/revoke/session_123")).toBe("api:admin");
  });
});

describe("hasPermission", () => {
  it("admin has wildcard access", () => {
    expect(hasPermission("admin", "anything")).toBe(true);
    expect(hasPermission("admin", "api:admin")).toBe(true);
  });

  it("user has chat permission", () => {
    expect(hasPermission("user", "api:chat")).toBe(true);
  });

  it("user lacks admin permission", () => {
    expect(hasPermission("user", "api:admin")).toBe(false);
  });

  it("readonly lacks chat permission", () => {
    expect(hasPermission("readonly", "api:chat")).toBe(false);
  });

  it("readonly has read permissions", () => {
    expect(hasPermission("readonly", "api:sessions:read")).toBe(true);
    expect(hasPermission("readonly", "api:agents:read")).toBe(true);
  });

  it("unknown role returns false", () => {
    expect(hasPermission("unknown", "api:chat")).toBe(false);
  });

  it("uses custom roles when provided", () => {
    const custom = { tester: ["api:chat", "api:test"] };
    expect(hasPermission("tester", "api:chat", custom)).toBe(true);
    expect(hasPermission("tester", "api:admin", custom)).toBe(false);
    // Custom roles replace default — admin from defaults not available
    expect(hasPermission("admin", "anything", custom)).toBe(false);
  });
});
