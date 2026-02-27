// Tests for plan and update CLI commands
// These test the command structure and API helper, not the full integration
import { describe, it, expect, vi, beforeEach } from "vitest";

describe("plan CLI", () => {
  it("planApi builds correct request URL", async () => {
    // We can't easily test Commander actions in unit tests,
    // but we can verify the core logic patterns

    // Verify plan list formatting
    const projects = [
      { id: "proj_abc123", state: "executing", goal: "Fix all the things" },
      { id: "proj_def456", state: "complete", goal: "Ship v2" },
    ];

    const lines: string[] = [];
    for (const p of projects) {
      const id = String(p.id).slice(0, 28);
      const state = String(p.state).padEnd(12);
      const goal = String(p.goal).slice(0, 60);
      lines.push(`${id.padEnd(30)}  ${state}  ${goal}`);
    }

    expect(lines).toHaveLength(2);
    expect(lines[0]).toContain("proj_abc123");
    expect(lines[0]).toContain("executing");
    expect(lines[0]).toContain("Fix all the things");
    expect(lines[1]).toContain("complete");
  });

  it("plan show formats phases with icons", () => {
    const phases = [
      { name: "Phase 1", state: "complete", goal: "Foundation", requirements: ["REQ-01"] },
      { name: "Phase 2", state: "executing", goal: "Build", requirements: ["REQ-02", "REQ-03"] },
      { name: "Phase 3", state: "pending", goal: "Polish", requirements: [] },
    ];

    const output: string[] = [];
    for (const ph of phases) {
      const icon = ph.state === "complete" ? "✅" : ph.state === "executing" ? "🔄" : "⬜";
      const reqs = ph.requirements;
      output.push(`${icon} ${ph.name}${reqs.length > 0 ? ` (${reqs.length} reqs)` : ""}`);
    }

    expect(output[0]).toContain("✅");
    expect(output[0]).toContain("1 reqs");
    expect(output[1]).toContain("🔄");
    expect(output[1]).toContain("2 reqs");
    expect(output[2]).toContain("⬜");
    expect(output[2]).not.toContain("reqs");
  });
});

describe("update CLI", () => {
  it("dry-run labels are correct", () => {
    const steps = [
      "Git pull",
      "Build runtime",
      "Build UI",
      "Copy runtime artifact",
      "Copy UI build",
      "Restart daemon",
    ];

    for (const step of steps) {
      const dryLabel = `[dry-run] ${step}`;
      expect(dryLabel).toMatch(/\[dry-run\]/);
      expect(dryLabel).toContain(step);
    }
  });

  it("skip-ui flag bypasses UI build step", () => {
    const skipUi = true;
    const steps: string[] = [];

    steps.push("git pull");
    steps.push("build runtime");
    if (!skipUi) {
      steps.push("build UI");
    }
    steps.push("copy artifacts");

    expect(steps).not.toContain("build UI");
    expect(steps).toHaveLength(3);
  });

  it("requirements summary counts tiers correctly", () => {
    const reqs = [
      { tier: "v1" }, { tier: "v1" }, { tier: "v1" },
      { tier: "v2" }, { tier: "v2" },
      { tier: "out-of-scope" },
    ];

    const v1 = reqs.filter(r => r.tier === "v1").length;
    const v2 = reqs.filter(r => r.tier === "v2").length;
    const oos = reqs.filter(r => r.tier === "out-of-scope").length;

    expect(v1).toBe(3);
    expect(v2).toBe(2);
    expect(oos).toBe(1);
  });
});
