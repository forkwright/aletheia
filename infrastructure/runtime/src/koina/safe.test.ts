// trySafe / trySafeAsync tests
import { describe, expect, it } from "vitest";
import { trySafe, trySafeAsync } from "./safe.js";

describe("trySafe", () => {
  it("returns function result on success", () => {
    expect(trySafe("test", () => 42, 0)).toBe(42);
  });

  it("returns fallback on throw", () => {
    expect(trySafe("test", () => { throw new Error("boom"); }, "default")).toBe("default");
  });

  it("returns null fallback", () => {
    expect(trySafe("test", () => { throw new Error("boom"); }, null)).toBeNull();
  });
});

describe("trySafeAsync", () => {
  it("returns promise result on success", async () => {
    expect(await trySafeAsync("test", async () => 42, 0)).toBe(42);
  });

  it("returns fallback on rejection", async () => {
    expect(await trySafeAsync("test", async () => { throw new Error("boom"); }, "default")).toBe("default");
  });
});
