// Crypto utilities tests
import { describe, expect, it } from "vitest";
import { generateId, generateSessionKey } from "./crypto.js";

describe("generateId", () => {
  it("generates 24-char hex without prefix", () => {
    const id = generateId();
    expect(id).toMatch(/^[0-9a-f]{24}$/);
  });

  it("prepends prefix with underscore", () => {
    const id = generateId("msg");
    expect(id).toMatch(/^msg_[0-9a-f]{24}$/);
  });

  it("generates unique IDs", () => {
    const ids = new Set(Array.from({ length: 100 }, () => generateId()));
    expect(ids.size).toBe(100);
  });

  it("handles empty string prefix (no prefix)", () => {
    const id = generateId("");
    expect(id).toMatch(/^[0-9a-f]{24}$/);
  });
});

describe("generateSessionKey", () => {
  it("prefixes with ses_", () => {
    const key = generateSessionKey();
    expect(key).toMatch(/^ses_[0-9a-f]{24}$/);
  });
});
