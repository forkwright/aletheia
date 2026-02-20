// Version module tests
import { describe, it, expect } from "vitest";
import { getVersion } from "./version.js";

describe("getVersion", () => {
  it("returns a semver string", () => {
    const version = getVersion();
    expect(version).toMatch(/^\d+\.\d+\.\d+/);
  });

  it("returns consistent results", () => {
    expect(getVersion()).toBe(getVersion());
  });
});
