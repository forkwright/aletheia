// Reversibility module tests
import { describe, expect, it } from "vitest";
import { buildSimulationPrompt, getReversibility, requiresSimulation } from "./reversibility.js";

describe("getReversibility", () => {
  it("returns reversible for read-only tools", () => {
    expect(getReversibility("file_read")).toBe("reversible");
    expect(getReversibility("grep")).toBe("reversible");
    expect(getReversibility("find")).toBe("reversible");
    expect(getReversibility("ls")).toBe("reversible");
    expect(getReversibility("web_fetch")).toBe("reversible");
    expect(getReversibility("web_search")).toBe("reversible");
    expect(getReversibility("mem0_search")).toBe("reversible");
  });

  it("returns irreversible for write tools", () => {
    expect(getReversibility("exec")).toBe("irreversible");
    expect(getReversibility("file_write")).toBe("irreversible");
    expect(getReversibility("file_edit")).toBe("irreversible");
    expect(getReversibility("message")).toBe("irreversible");
    expect(getReversibility("voice_reply")).toBe("irreversible");
    expect(getReversibility("sessions_send")).toBe("irreversible");
  });

  it("returns destructive for fact_retract", () => {
    expect(getReversibility("fact_retract")).toBe("destructive");
  });

  it("defaults to reversible for unknown tools", () => {
    expect(getReversibility("unknown_tool")).toBe("reversible");
  });
});

describe("requiresSimulation", () => {
  it("returns true for destructive tools", () => {
    expect(requiresSimulation("fact_retract", {})).toBe(true);
  });

  it("returns true for message and voice_reply", () => {
    expect(requiresSimulation("message", {})).toBe(true);
    expect(requiresSimulation("voice_reply", {})).toBe(true);
  });

  it("returns true for exec with dangerous commands", () => {
    expect(requiresSimulation("exec", { command: "rm -rf /" })).toBe(true);
    expect(requiresSimulation("exec", { command: "dd if=/dev/zero" })).toBe(true);
    expect(requiresSimulation("exec", { command: "mkfs.ext4 /dev/sda" })).toBe(true);
    expect(requiresSimulation("exec", { command: "shutdown -h now" })).toBe(true);
    expect(requiresSimulation("exec", { command: "reboot" })).toBe(true);
  });

  it("returns false for exec with safe commands", () => {
    expect(requiresSimulation("exec", { command: "ls -la" })).toBe(false);
    expect(requiresSimulation("exec", { command: "cat file.txt" })).toBe(false);
  });

  it("returns false for reversible tools", () => {
    expect(requiresSimulation("file_read", {})).toBe(false);
    expect(requiresSimulation("grep", {})).toBe(false);
  });

  it("returns false for irreversible non-messaging tools", () => {
    expect(requiresSimulation("file_write", {})).toBe(false);
    expect(requiresSimulation("file_edit", {})).toBe(false);
  });

  it("handles missing command in exec", () => {
    expect(requiresSimulation("exec", {})).toBe(false);
  });
});

describe("buildSimulationPrompt", () => {
  it("includes tool name and reversibility", () => {
    const prompt = buildSimulationPrompt("message", { text: "hello" });
    expect(prompt).toContain('"message"');
    expect(prompt).toContain("irreversible");
  });

  it("truncates long input to 500 chars", () => {
    const longInput = { data: "x".repeat(1000) };
    const prompt = buildSimulationPrompt("exec", longInput);
    expect(prompt.length).toBeLessThan(1500);
  });

  it("includes assessment questions", () => {
    const prompt = buildSimulationPrompt("fact_retract", {});
    expect(prompt).toContain("Can it be undone");
    expect(prompt).toContain("proceed");
  });
});
