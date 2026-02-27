// Tests for bootstrap-loader — loadBootstrapAnchor() and writeBootstrapAnchor()
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Mock } from "vitest";

vi.mock("../koina/fs.js", () => ({
  readJson: vi.fn(),
  writeJson: vi.fn(),
  exists: vi.fn(),
}));

describe("loadBootstrapAnchor", () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it("throws ConfigError with CONFIG_ANCHOR_NOT_FOUND when readJson returns null", async () => {
    const { readJson } = await import("../koina/fs.js");
    (readJson as Mock).mockReturnValue(null);
    const { loadBootstrapAnchor } = await import("./bootstrap-loader.js");

    expect(() => loadBootstrapAnchor()).toThrow(
      expect.objectContaining({ code: "CONFIG_ANCHOR_NOT_FOUND" }),
    );
  });

  it("throws ConfigError with CONFIG_ANCHOR_INVALID when JSON is missing required fields", async () => {
    const { readJson } = await import("../koina/fs.js");
    (readJson as Mock).mockReturnValue({ nousDir: "/somewhere" }); // missing deployDir
    const { loadBootstrapAnchor } = await import("./bootstrap-loader.js");

    expect(() => loadBootstrapAnchor()).toThrow(
      expect.objectContaining({ code: "CONFIG_ANCHOR_INVALID" }),
    );
  });

  it("returns { anchor, path } with correct nousDir and deployDir when JSON is valid", async () => {
    const { readJson } = await import("../koina/fs.js");
    (readJson as Mock).mockReturnValue({
      nousDir: "/data/nous",
      deployDir: "/data/deploy",
    });
    const { loadBootstrapAnchor } = await import("./bootstrap-loader.js");

    const result = loadBootstrapAnchor();
    expect(result.anchor.nousDir).toBe("/data/nous");
    expect(result.anchor.deployDir).toBe("/data/deploy");
    expect(typeof result.path).toBe("string");
    expect(result.path.endsWith("anchor.json")).toBe(true);
  });

  it("does not throw when anchor has unknown extra keys (forward-compat)", async () => {
    const { readJson } = await import("../koina/fs.js");
    (readJson as Mock).mockReturnValue({
      nousDir: "/data/nous",
      deployDir: "/data/deploy",
      futureKey: "some future value",
    });
    const { loadBootstrapAnchor } = await import("./bootstrap-loader.js");

    expect(() => loadBootstrapAnchor()).not.toThrow();
  });
});

describe("writeBootstrapAnchor", () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it("calls writeJson with correct structure including $comment field", async () => {
    const { writeJson } = await import("../koina/fs.js");
    const { writeBootstrapAnchor } = await import("./bootstrap-loader.js");

    writeBootstrapAnchor("/my/nous", "/my/deploy");

    expect(writeJson as Mock).toHaveBeenCalledOnce();
    const [, data] = (writeJson as Mock).mock.calls[0] as [string, Record<string, unknown>];
    expect(data.$comment).toBeDefined();
    expect(data.nousDir).toBe("/my/nous");
    expect(data.deployDir).toBe("/my/deploy");
  });
});
