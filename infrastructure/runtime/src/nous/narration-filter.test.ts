import { describe, it, expect } from "vitest";
import { isNarration, NarrationFilter } from "./narration-filter.js";

describe("isNarration", () => {
  const positives = [
    "Let me check the logs for errors.",
    "I'll read the file now.",
    "I need to search for that function.",
    "I'm going to look at the configuration.",
    "Now let me examine the database schema.",
    "First, let me review the test results.",
    "OK, let me find the relevant code.",
    "Alright, I'll check the API endpoint.",
    "Looking at the database records.",
    "Checking the file permissions.",
    "Reading the configuration file now.",
    "Searching for the error pattern.",
    "I should verify the deployment status.",
    "I want to examine the logs more closely.",
    "I will update the configuration.",
    "Verifying the test results now.",
  ];

  const negatives = [
    "Let me know if you need anything else.",
    "The function checks for null values.",
    "Here are the results you asked for.",
    "short",
    "This is a very long paragraph that goes on and on about various topics and definitely should not be classified as narration because it is over two hundred characters long and contains actual substantive content that the user would want to see in the response text rather than being hidden away.",
    "The error occurs because the database connection times out.",
    "You should update the package to version 3.2.",
    "I recommend using TypeScript for this project.",
    "Based on my analysis, the root cause is...",
    "",
  ];

  for (const s of positives) {
    it(`detects narration: "${s.slice(0, 50)}..."`, () => {
      expect(isNarration(s)).toBe(true);
    });
  }

  for (const s of negatives) {
    it(`passes through: "${s.slice(0, 50)}..."`, () => {
      expect(isNarration(s)).toBe(false);
    });
  }
});

describe("NarrationFilter", () => {
  it("suppresses narration at start of response", () => {
    const filter = new NarrationFilter();
    // Sentence boundary requires trailing whitespace — "Here are the results." stays buffered
    const events = filter.feed("Let me check the logs. Here are the results. ");
    // First sentence is narration → thinking_delta
    // Second sentence is not → text_delta (+ rest)
    expect(events).toHaveLength(2);
    expect(events[0]!.type).toBe("thinking_delta");
    expect(events[0]!.text).toContain("Let me check the logs.");
    expect(events[1]!.type).toBe("text_delta");
    expect(events[1]!.text).toContain("Here are the results.");
  });

  it("passes through non-narration from the start", () => {
    const filter = new NarrationFilter();
    const events = filter.feed("The error is in line 42. Let me check the logs.");
    // First sentence is not narration → text_delta with rest of buffer
    expect(events).toHaveLength(1);
    expect(events[0]!.type).toBe("text_delta");
    expect(events[0]!.text).toContain("The error is in line 42.");
    expect(events[0]!.text).toContain("Let me check the logs.");
  });

  it("buffers incomplete sentences", () => {
    const filter = new NarrationFilter();
    // "Let me check" — no sentence boundary yet
    const events1 = filter.feed("Let me check");
    expect(events1).toHaveLength(0);

    // Complete the sentence + start a new one (need trailing space for boundary)
    const events2 = filter.feed(" the logs. Here are the results. ");
    expect(events2).toHaveLength(2);
    expect(events2[0]!.type).toBe("thinking_delta");
    expect(events2[1]!.type).toBe("text_delta");
  });

  it("stops filtering after first non-narration sentence", () => {
    const filter = new NarrationFilter();
    filter.feed("The answer is 42. ");
    // Now everything should pass through
    const events = filter.feed("Let me check more stuff.");
    expect(events).toHaveLength(1);
    expect(events[0]!.type).toBe("text_delta");
  });

  it("handles flush of narration buffer", () => {
    const filter = new NarrationFilter();
    filter.feed("Let me check the logs");
    const events = filter.flush();
    expect(events).toHaveLength(1);
    expect(events[0]!.type).toBe("thinking_delta");
  });

  it("handles flush of non-narration buffer", () => {
    const filter = new NarrationFilter();
    filter.feed("Here are the results");
    const events = filter.flush();
    expect(events).toHaveLength(1);
    expect(events[0]!.type).toBe("text_delta");
  });

  it("handles empty flush", () => {
    const filter = new NarrationFilter();
    const events = filter.flush();
    expect(events).toHaveLength(0);
  });

  it("suppresses multiple consecutive narration sentences", () => {
    const filter = new NarrationFilter();
    const events = filter.feed("Let me check the logs. I'll read the configuration file. The root cause is a missing import. ");
    expect(events).toHaveLength(3);
    expect(events[0]!.type).toBe("thinking_delta");
    expect(events[1]!.type).toBe("thinking_delta");
    expect(events[2]!.type).toBe("text_delta");
  });

  it("streams chunk by chunk correctly", () => {
    const filter = new NarrationFilter();
    const allEvents: Array<{ type: string; text: string }> = [];

    // Simulate streaming: "Let me check the logs. The error is clear."
    for (const chunk of ["Let ", "me ch", "eck the lo", "gs. ", "The er", "ror is cl", "ear."]) {
      allEvents.push(...filter.feed(chunk));
    }
    allEvents.push(...filter.flush());

    // Should have narration suppressed, then text passed through
    const thinkingEvents = allEvents.filter((e) => e.type === "thinking_delta");
    const textEvents = allEvents.filter((e) => e.type === "text_delta");
    expect(thinkingEvents.length).toBeGreaterThan(0);
    expect(textEvents.length).toBeGreaterThan(0);

    // Reconstruct: all text should be present
    const allText = allEvents.map((e) => e.text).join("");
    expect(allText).toContain("Let me check the logs.");
    expect(allText).toContain("The error is clear.");
  });
});
