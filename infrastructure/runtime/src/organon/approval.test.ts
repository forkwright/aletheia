import { describe, it, expect } from "vitest";
import { requiresApproval, ApprovalGate } from "./approval.js";

describe("requiresApproval", () => {
  it("autonomous mode never requires approval", () => {
    const result = requiresApproval("exec", { command: "rm -rf /" }, "autonomous");
    expect(result.required).toBe(false);
  });

  it("guarded mode pauses on rm -rf", () => {
    const result = requiresApproval("exec", { command: "rm -rf /tmp/test" }, "guarded");
    expect(result.required).toBe(true);
    expect(result.risk).toBe("destructive");
  });

  it("guarded mode pauses on git push --force", () => {
    const result = requiresApproval("exec", { command: "git push --force origin main" }, "guarded");
    expect(result.required).toBe(true);
  });

  it("guarded mode pauses on DROP TABLE", () => {
    const result = requiresApproval("exec", { command: "psql -c 'DROP TABLE users'" }, "guarded");
    expect(result.required).toBe(true);
  });

  it("guarded mode allows safe commands", () => {
    expect(requiresApproval("exec", { command: "ls -la" }, "guarded").required).toBe(false);
    expect(requiresApproval("exec", { command: "git status" }, "guarded").required).toBe(false);
    expect(requiresApproval("exec", { command: "npm install" }, "guarded").required).toBe(false);
    expect(requiresApproval("exec", { command: "cat file.txt" }, "guarded").required).toBe(false);
  });

  it("guarded mode allows read tools", () => {
    expect(requiresApproval("file_read", { path: "/etc/passwd" }, "guarded").required).toBe(false);
    expect(requiresApproval("grep", { pattern: "test" }, "guarded").required).toBe(false);
    expect(requiresApproval("ls", { path: "/" }, "guarded").required).toBe(false);
  });

  it("guarded mode allows file writes", () => {
    expect(requiresApproval("file_write", { path: "test.txt" }, "guarded").required).toBe(false);
    expect(requiresApproval("file_edit", { path: "test.txt" }, "guarded").required).toBe(false);
  });

  it("guarded mode pauses on message send", () => {
    expect(requiresApproval("message", { to: "+1234567890", text: "hi" }, "guarded").required).toBe(true);
  });

  it("guarded mode pauses on fact_retract (destructive)", () => {
    expect(requiresApproval("fact_retract", { id: "123" }, "guarded").required).toBe(true);
  });

  it("supervised mode pauses on writes", () => {
    expect(requiresApproval("file_write", { path: "test.txt" }, "supervised").required).toBe(true);
    expect(requiresApproval("exec", { command: "echo hi" }, "supervised").required).toBe(true);
  });

  it("supervised mode allows reads", () => {
    expect(requiresApproval("file_read", {}, "supervised").required).toBe(false);
    expect(requiresApproval("grep", {}, "supervised").required).toBe(false);
    expect(requiresApproval("mem0_search", {}, "supervised").required).toBe(false);
  });

  it("session allow list bypasses approval", () => {
    const allowList = new Set(["exec"]);
    const result = requiresApproval("exec", { command: "rm -rf /" }, "guarded", allowList);
    expect(result.required).toBe(false);
  });
});

describe("ApprovalGate", () => {
  it("resolves approval", async () => {
    const gate = new ApprovalGate();
    const promise = gate.waitForApproval("turn1", "tool1", "exec", {}, "irreversible");

    // Resolve from another "thread"
    setTimeout(() => {
      gate.resolveApproval("turn1", "tool1", { decision: "approve" });
    }, 10);

    const result = await promise;
    expect(result.decision).toBe("approve");
  });

  it("resolves denial", async () => {
    const gate = new ApprovalGate();
    const promise = gate.waitForApproval("turn1", "tool1", "exec", {}, "destructive");

    setTimeout(() => {
      gate.resolveApproval("turn1", "tool1", { decision: "deny" });
    }, 10);

    const result = await promise;
    expect(result.decision).toBe("deny");
  });

  it("returns false for unknown approvals", () => {
    const gate = new ApprovalGate();
    expect(gate.resolveApproval("unknown", "unknown", { decision: "approve" })).toBe(false);
  });

  it("tracks session allow lists", () => {
    const gate = new ApprovalGate();
    expect(gate.getSessionAllowList("s1")).toBeUndefined();
    gate.addToSessionAllowList("s1", "exec");
    expect(gate.getSessionAllowList("s1")?.has("exec")).toBe(true);
  });

  it("cancels on abort signal", async () => {
    const gate = new ApprovalGate();
    const controller = new AbortController();

    const promise = gate.waitForApproval("turn1", "tool1", "exec", {}, "irreversible", controller.signal);

    setTimeout(() => controller.abort(), 10);

    await expect(promise).rejects.toThrow("Approval cancelled");
  });
});
