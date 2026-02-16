// Browser tool tests — definition and error handling
import { describe, it, expect, vi, beforeEach } from "vitest";
import { browserTool, closeBrowser } from "./browser.js";

// Mock SSRF guard
vi.mock("./ssrf-guard.js", () => ({
  validateUrl: vi.fn().mockResolvedValue(undefined),
}));

// Mock playwright-core to avoid needing a real browser
vi.mock("playwright-core", () => ({
  chromium: {
    launch: vi.fn().mockRejectedValue(new Error("ENOENT: chromium not found")),
  },
}));

describe("browserTool", () => {
  it("has valid definition", () => {
    expect(browserTool.definition.name).toBe("browser");
    expect(browserTool.definition.input_schema.required).toContain("url");
  });

  it("returns actionable error when Chromium not found", async () => {
    const result = await browserTool.execute({ url: "https://example.com" });
    expect(result).toContain("Chromium not found");
  });

  it("has extract and screenshot actions documented", () => {
    const schema = browserTool.definition.input_schema;
    const actionProp = (schema.properties as Record<string, { enum?: string[] }>)["action"];
    expect(actionProp?.enum).toContain("navigate");
    expect(actionProp?.enum).toContain("screenshot");
    expect(actionProp?.enum).toContain("extract");
  });
});

describe("closeBrowser", () => {
  it("is exported as a function", () => {
    expect(typeof closeBrowser).toBe("function");
  });
});
