// Sub-agent role resolution tests
import { describe, expect, it } from "vitest";
import { resolveRole, ROLE_NAMES, SUB_AGENT_ROLES } from "./sub-agent-roles.js";

describe("resolveRole", () => {
  it("resolves known roles", () => {
    for (const name of ROLE_NAMES) {
      const role = resolveRole(name);
      expect(role).not.toBeNull();
      expect(role!.maxTurns).toBeGreaterThan(0);
      expect(role!.maxTokenBudget).toBeGreaterThan(0);
      expect(role!.systemPromptTemplate).toBeTruthy();
      expect(Array.isArray(role!.tools)).toBe(true);
    }
  });

  it("returns null for unknown role", () => {
    expect(resolveRole("nonexistent")).toBeNull();
    expect(resolveRole("")).toBeNull();
  });
});

describe("SUB_AGENT_ROLES", () => {
  it("has entries for all ROLE_NAMES", () => {
    for (const name of ROLE_NAMES) {
      expect(SUB_AGENT_ROLES[name]).toBeDefined();
    }
  });

  it("transforms RoleConfig to SubAgentRole correctly", () => {
    const coder = SUB_AGENT_ROLES["coder"]!;
    expect(coder.tools).toContain("read");
    expect(coder.tools).toContain("write");
    expect(typeof coder.systemPromptTemplate).toBe("string");
  });
});
