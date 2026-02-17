// Cross-agent deliberation protocol tests
import { describe, it, expect } from "vitest";
import { createDeliberateTool } from "./deliberate.js";
import type { InboundMessage, TurnOutcome } from "../../nous/manager.js";

function makeDispatcher(responses: Record<string, string>) {
  const calls: InboundMessage[] = [];
  return {
    dispatcher: {
      handleMessage: async (msg: InboundMessage): Promise<TurnOutcome> => {
        calls.push(msg);
        const text = responses[msg.nousId] ?? "default response";
        return {
          sessionId: `session_${msg.nousId}`,
          text,
          toolCalls: 0,
          inputTokens: 100,
          outputTokens: 50,
        };
      },
    },
    calls,
  };
}

const ctx = { nousId: "syn", sessionId: "s1", workspace: "/tmp" };

describe("deliberate", () => {
  it("runs full pose → critique → revise → synthesize with 2 agents", async () => {
    const { dispatcher, calls } = makeDispatcher({
      eiron: "My position on this topic is...",
      arbor: "I find several issues with that position...",
    });

    const tool = createDeliberateTool(dispatcher);
    const raw = await tool.execute(
      { topic: "Should we use Rust or Go?", agents: ["eiron", "arbor"] },
      ctx,
    );
    const result = JSON.parse(raw);

    expect(result.deliberationId).toBeDefined();
    expect(result.topic).toBe("Should we use Rust or Go?");
    expect(result.participants).toContain("syn");
    expect(result.participants).toContain("eiron");
    expect(result.participants).toContain("arbor");
    expect(result.phases).toHaveLength(4);
    expect(result.phases[0].phase).toBe("pose");
    expect(result.phases[0].agent).toBe("eiron");
    expect(result.phases[1].phase).toBe("critique");
    expect(result.phases[1].agent).toBe("arbor");
    expect(result.phases[2].phase).toBe("revise");
    expect(result.phases[2].agent).toBe("eiron");
    expect(result.phases[3].phase).toBe("synthesize");
    expect(result.synthesis).toBeDefined();
    expect(result.tokens.input).toBeGreaterThan(0);

    // Verify each phase sent correct prompts
    expect(calls[0].nousId).toBe("eiron"); // pose
    expect(calls[0].text).toContain("POSE PHASE");
    expect(calls[1].nousId).toBe("arbor"); // critique
    expect(calls[1].text).toContain("CRITIQUE PHASE");
    expect(calls[2].nousId).toBe("eiron"); // revise
    expect(calls[2].text).toContain("REVISE PHASE");
    expect(calls[3].nousId).toBe("eiron"); // synthesize (falls back to first participant with 2 agents)
    expect(calls[3].text).toContain("SYNTHESIS PHASE");
  });

  it("handles single agent deliberation with caller as critic", async () => {
    const { dispatcher, calls } = makeDispatcher({
      demiurge: "Creative approach: we should...",
    });

    const tool = createDeliberateTool(dispatcher);
    const raw = await tool.execute(
      { topic: "Best woodworking joint for this project", agents: ["demiurge"] },
      ctx,
    );
    const result = JSON.parse(raw);

    expect(result.phases).toHaveLength(4);
    expect(result.phases[0].agent).toBe("demiurge"); // pose
    expect(result.phases[1].agent).toBe("syn"); // critique (caller)
    expect(result.phases[1].content).toContain("Caller acts as critic");
    expect(result.phases[2].agent).toBe("demiurge"); // revise
    // Only 2 dispatcher calls (pose, revise — critique and synthesis are caller)
    expect(calls).toHaveLength(2);
  });

  it("rejects empty agents list", async () => {
    const { dispatcher } = makeDispatcher({});
    const tool = createDeliberateTool(dispatcher);
    const raw = await tool.execute(
      { topic: "test", agents: [] },
      ctx,
    );
    const result = JSON.parse(raw);
    expect(result.error).toBeDefined();
  });

  it("removes self from agents list", async () => {
    const { dispatcher, calls } = makeDispatcher({
      eiron: "response",
    });

    const tool = createDeliberateTool(dispatcher);
    const raw = await tool.execute(
      { topic: "test", agents: ["syn", "eiron"] },
      ctx,
    );
    const result = JSON.parse(raw);

    // syn should be removed, only eiron participates
    expect(result.phases[0].agent).toBe("eiron");
    expect(calls.every((c) => c.nousId !== "syn")).toBe(true);
  });

  it("continues when critique phase fails", async () => {
    let callCount = 0;
    const dispatcher = {
      handleMessage: async (msg: InboundMessage): Promise<TurnOutcome> => {
        callCount++;
        // Second call (critique) times out
        if (callCount === 2) {
          throw new Error("Timeout after 5s");
        }
        return {
          sessionId: "s",
          text: `response ${callCount}`,
          toolCalls: 0,
          inputTokens: 50,
          outputTokens: 25,
        };
      },
    };

    const tool = createDeliberateTool(dispatcher);
    const raw = await tool.execute(
      { topic: "test", agents: ["eiron", "arbor"], timeoutPerPhase: 5 },
      ctx,
    );
    const result = JSON.parse(raw);

    // Should still complete — critique failure is non-fatal
    expect(result.phases).toHaveLength(4);
    expect(result.phases[1].content).toContain("skipped");
  });

  it("returns error without dispatcher", async () => {
    const tool = createDeliberateTool(undefined);
    const raw = await tool.execute(
      { topic: "test", agents: ["eiron"] },
      ctx,
    );
    const result = JSON.parse(raw);
    expect(result.error).toBe("Agent dispatch not available");
  });

  it("posts results to blackboard when store is available", async () => {
    const written: Array<{ key: string; value: string }> = [];
    const auditCalls: unknown[] = [];
    const { dispatcher } = makeDispatcher({ eiron: "my position" });
    dispatcher.store = {
      blackboardWrite: (key: string, value: string, author: string, ttl: number) => {
        written.push({ key, value });
        return "bb_1";
      },
      recordCrossAgentCall: (args: unknown) => {
        auditCalls.push(args);
        return 1;
      },
    } as any;

    const tool = createDeliberateTool(dispatcher);
    await tool.execute(
      { topic: "test topic", agents: ["eiron"] },
      ctx,
    );

    expect(written).toHaveLength(1);
    expect(written[0].key).toContain("deliberation:");
    expect(written[0].value).toContain("DELIBERATION: test topic");
    expect(auditCalls).toHaveLength(1);
  });
});
