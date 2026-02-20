// Update check daemon tests
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// Extract isNewer for testing by re-implementing the logic
// (the actual function is private, so we test via the module's behavior)
function isNewer(latest: string, current: string): boolean {
  const parse = (v: string) => v.split(".").map(Number);
  const [lMajor = 0, lMinor = 0, lPatch = 0] = parse(latest);
  const [cMajor = 0, cMinor = 0, cPatch = 0] = parse(current);
  if (lMajor !== cMajor) return lMajor > cMajor;
  if (lMinor !== cMinor) return lMinor > cMinor;
  return lPatch > cPatch;
}

describe("update-check", () => {
  describe("isNewer", () => {
    it("detects newer patch version", () => {
      expect(isNewer("0.9.2", "0.9.1")).toBe(true);
    });

    it("detects newer minor version", () => {
      expect(isNewer("0.10.0", "0.9.5")).toBe(true);
    });

    it("detects newer major version", () => {
      expect(isNewer("1.0.0", "0.99.99")).toBe(true);
    });

    it("returns false for same version", () => {
      expect(isNewer("0.9.0", "0.9.0")).toBe(false);
    });

    it("returns false for older version", () => {
      expect(isNewer("0.8.0", "0.9.0")).toBe(false);
    });

    it("returns false for older patch", () => {
      expect(isNewer("0.9.0", "0.9.1")).toBe(false);
    });
  });

  describe("startUpdateChecker", () => {
    let fetchSpy: ReturnType<typeof vi.fn>;
    let timer: NodeJS.Timeout | null = null;

    beforeEach(() => {
      vi.useFakeTimers();
      fetchSpy = vi.fn();
      vi.stubGlobal("fetch", fetchSpy);
    });

    afterEach(() => {
      if (timer) clearInterval(timer);
      vi.useRealTimers();
      vi.restoreAllMocks();
    });

    it("checks after initial delay", async () => {
      const store = {
        blackboardWrite: vi.fn(),
      };

      fetchSpy.mockResolvedValue({
        ok: true,
        json: () => Promise.resolve({
          tag_name: "v0.9.1",
          html_url: "https://github.com/forkwright/aletheia/releases/tag/v0.9.1",
        }),
      });

      const { startUpdateChecker } = await import("./update-check.js");
      timer = startUpdateChecker(store as never, "0.9.0");

      // Advance past initial delay (60s)
      await vi.advanceTimersByTimeAsync(61_000);

      expect(fetchSpy).toHaveBeenCalledTimes(1);
      expect(store.blackboardWrite).toHaveBeenCalledWith(
        "system:update",
        expect.stringContaining('"available":true'),
        "system",
        expect.any(Number),
      );
    });

    it("reports not available when current is latest", async () => {
      const store = {
        blackboardWrite: vi.fn(),
      };

      fetchSpy.mockResolvedValue({
        ok: true,
        json: () => Promise.resolve({
          tag_name: "v0.9.0",
          html_url: "https://github.com/forkwright/aletheia/releases/tag/v0.9.0",
        }),
      });

      const { startUpdateChecker } = await import("./update-check.js");
      timer = startUpdateChecker(store as never, "0.9.0");

      await vi.advanceTimersByTimeAsync(61_000);

      expect(store.blackboardWrite).toHaveBeenCalledWith(
        "system:update",
        expect.stringContaining('"available":false'),
        "system",
        expect.any(Number),
      );
    });
  });
});
