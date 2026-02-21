// Message tool tests
import { describe, expect, it, vi } from "vitest";
import { createMessageTool } from "./message.js";

describe("createMessageTool", () => {
  it("has valid definition", () => {
    const tool = createMessageTool();
    expect(tool.definition.name).toBe("message");
    expect(tool.definition.input_schema.required).toContain("to");
    expect(tool.definition.input_schema.required).toContain("text");
  });

  it("returns error when no sender configured", async () => {
    const tool = createMessageTool();
    const result = await tool.execute({ to: "+1234567890", text: "hello" });
    expect(JSON.parse(result).error).toContain("Signal not connected");
  });

  it("sends message via sender", async () => {
    const sender = { send: vi.fn().mockResolvedValue(undefined) };
    const tool = createMessageTool({ sender });
    const result = await tool.execute({ to: "+1234567890", text: "hello" });
    const parsed = JSON.parse(result);
    expect(parsed.sent).toBe(true);
    expect(parsed.to).toBe("+1234567890");
    expect(sender.send).toHaveBeenCalledWith("+1234567890", "hello");
  });

  it("rejects recipients not in allowlist", async () => {
    const sender = { send: vi.fn() };
    const tool = createMessageTool({ sender, allowedRecipients: ["+9999"] });
    const result = await tool.execute({ to: "+1234567890", text: "hello" });
    expect(JSON.parse(result).error).toContain("not in allowlist");
    expect(sender.send).not.toHaveBeenCalled();
  });

  it("allows recipients in allowlist", async () => {
    const sender = { send: vi.fn().mockResolvedValue(undefined) };
    const tool = createMessageTool({ sender, allowedRecipients: ["+1234567890"] });
    const result = await tool.execute({ to: "+1234567890", text: "hello" });
    expect(JSON.parse(result).sent).toBe(true);
  });

  it("truncates long messages", async () => {
    const sender = { send: vi.fn().mockResolvedValue(undefined) };
    const tool = createMessageTool({ sender, maxLength: 10 });
    const result = await tool.execute({ to: "+1234567890", text: "this is a very long message" });
    expect(JSON.parse(result).length).toBe(10);
    expect(sender.send).toHaveBeenCalledWith("+1234567890", "this is a ");
  });
});
