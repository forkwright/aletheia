// Interaction signal classification tests
import { describe, expect, it } from "vitest";
import { classifyInteraction } from "./interaction-signals.js";

describe("classifyInteraction", () => {
  describe("correction signals", () => {
    it.each([
      "no, that's wrong",
      "actually, I meant something else",
      "that's incorrect",
      "not what I wanted",
      "wrong.",
      "incorrect answer",
      "I said something different",
      "I meant the other one",
    ])("detects correction: %s", (text) => {
      const result = classifyInteraction(text);
      expect(result.signal).toBe("correction");
      expect(result.confidence).toBe(0.8);
    });
  });

  describe("approval signals", () => {
    it.each([
      "yes, that's right",
      "perfect",
      "exactly",
      "great work",
      "thanks!",
      "good job",
      "nice",
      "that's correct",
    ])("detects approval: %s", (text) => {
      const result = classifyInteraction(text);
      expect(result.signal).toBe("approval");
      expect(result.confidence).toBe(0.8);
    });
  });

  describe("followup signals", () => {
    it.each([
      "and also do this",
      "what about the other case",
      "now do the tests",
      "next, update the docs",
      "one more thing",
      "can you also fix the bug",
    ])("detects followup: %s", (text) => {
      const result = classifyInteraction(text);
      expect(result.signal).toBe("followup");
      expect(result.confidence).toBe(0.7);
    });
  });

  describe("escalation signals", () => {
    it.each([
      "this is urgent",
      "emergency fix needed",
      "asap please",
      "critical issue",
      "ask syn about this",
      "talk to chiron about my schedule",
    ])("detects escalation: %s", (text) => {
      const result = classifyInteraction(text);
      expect(result.signal).toBe("escalation");
      expect(result.confidence).toBe(0.7);
    });
  });

  describe("clarification signals", () => {
    it.each([
      "what do you mean by that",
      "can you explain further",
      "I don't understand",
      "what does that mean",
      "please clarify",
      "could you elaborate",
    ])("detects clarification: %s", (text) => {
      const result = classifyInteraction(text);
      expect(result.signal).toBe("clarification");
      expect(result.confidence).toBe(0.7);
    });
  });

  describe("topic change detection", () => {
    it("detects topic change via low word overlap with previous response", () => {
      const result = classifyInteraction(
        "How do I configure my router's firewall settings?",
        "The TypeScript compiler uses the tsconfig.json file to determine project settings and compilation options for your codebase.",
      );
      expect(result.signal).toBe("topic_change");
      expect(result.confidence).toBe(0.6);
    });

    it("does not flag topic change when previous response is short", () => {
      const result = classifyInteraction("something new", "ok");
      expect(result.signal).not.toBe("topic_change");
    });
  });

  describe("short question fallback", () => {
    it("classifies short questions as clarification", () => {
      const result = classifyInteraction("why?");
      expect(result.signal).toBe("clarification");
      expect(result.confidence).toBe(0.5);
    });
  });

  describe("neutral fallback", () => {
    it("returns neutral for ambiguous text", () => {
      const result = classifyInteraction("Here is the updated configuration file with the changes we discussed earlier.");
      expect(result.signal).toBe("neutral");
      expect(result.confidence).toBe(0.4);
    });
  });
});
