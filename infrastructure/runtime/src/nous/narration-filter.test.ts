import { describe, expect, it } from "vitest";
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
    // New Phase 4 patterns
    "Good call. Let me check the spec.",
    "Good point, I'll read the config.",
    "Now I need to verify the output.",
    "Now I should check the database.",
    "Let me also check the tests.",
    "Let me quickly verify the build.",
    "Time to check the deployment.",
    "Going to read the error logs.",
    "About to scan the directory.",
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
    "Good call on both counts.",
    "Now we have a working solution.",
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
    const events = filter.feed("Let me check the logs. Here are the results. ");
    expect(events).toHaveLength(2);
    expect(events[0]!.type).toBe("thinking_delta");
    expect(events[0]!.text).toContain("Let me check the logs.");
    expect(events[1]!.type).toBe("text_delta");
    expect(events[1]!.text).toContain("Here are the results.");
  });

  it("passes through non-narration from the start", () => {
    const filter = new NarrationFilter();
    const events = filter.feed("The error is in line 42. Here are the details. ");
    expect(events).toHaveLength(2);
    expect(events[0]!.type).toBe("text_delta");
    expect(events[0]!.text).toContain("The error is in line 42.");
    expect(events[1]!.type).toBe("text_delta");
    expect(events[1]!.text).toContain("Here are the details.");
  });

  it("buffers incomplete sentences", () => {
    const filter = new NarrationFilter();
    const events1 = filter.feed("Let me check");
    expect(events1).toHaveLength(0);

    const events2 = filter.feed(" the logs. Here are the results. ");
    expect(events2).toHaveLength(2);
    expect(events2[0]!.type).toBe("thinking_delta");
    expect(events2[1]!.type).toBe("text_delta");
  });

  it("catches narration AFTER non-narration sentences", () => {
    const filter = new NarrationFilter();
    const events = filter.feed("The answer is 42. Let me check the details. ");
    expect(events).toHaveLength(2);
    expect(events[0]!.type).toBe("text_delta");
    expect(events[0]!.text).toContain("The answer is 42.");
    expect(events[1]!.type).toBe("thinking_delta");
    expect(events[1]!.text).toContain("Let me check the details.");
  });

  it("catches narration sandwiched between non-narration", () => {
    const filter = new NarrationFilter();
    const events = filter.feed("Good news. Let me check the logs. The fix is simple. ");
    expect(events).toHaveLength(3);
    expect(events[0]!.type).toBe("text_delta");
    expect(events[1]!.type).toBe("thinking_delta");
    expect(events[2]!.type).toBe("text_delta");
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

    for (const chunk of ["Let ", "me ch", "eck the lo", "gs. ", "The er", "ror is cl", "ear."]) {
      allEvents.push(...filter.feed(chunk));
    }
    allEvents.push(...filter.flush());

    const thinkingEvents = allEvents.filter((e) => e.type === "thinking_delta");
    const textEvents = allEvents.filter((e) => e.type === "text_delta");
    expect(thinkingEvents.length).toBeGreaterThan(0);
    expect(textEvents.length).toBeGreaterThan(0);

    const allText = allEvents.map((e) => e.text).join("");
    expect(allText).toContain("Let me check the logs.");
    expect(allText).toContain("The error is clear.");
  });

  it("classifies new Phase 4 patterns mid-response", () => {
    const filter = new NarrationFilter();
    const events = filter.feed("Good call. Let me check the spec. Here is what I found. ");
    expect(events).toHaveLength(3);
    expect(events[0]!.type).toBe("text_delta");
    expect(events[1]!.type).toBe("thinking_delta");
    expect(events[2]!.type).toBe("text_delta");
  });

  it("handles interleaved narration and substantive content", () => {
    const filter = new NarrationFilter();
    const allEvents: Array<{ type: string; text: string }> = [];

    allEvents.push(...filter.feed("The config looks correct. "));
    allEvents.push(...filter.feed("Let me verify the build. "));
    allEvents.push(...filter.feed("Build succeeded with no errors. "));
    allEvents.push(...filter.feed("Now let me check the tests. "));
    allEvents.push(...filter.feed("All 42 tests pass. "));
    allEvents.push(...filter.flush());

    const types = allEvents.map((e) => e.type);
    expect(types).toEqual([
      "text_delta",      // The config looks correct.
      "thinking_delta",  // Let me verify the build.
      "text_delta",      // Build succeeded with no errors.
      "thinking_delta",  // Now let me check the tests.
      "text_delta",      // All 42 tests pass.
    ]);
  });
});
