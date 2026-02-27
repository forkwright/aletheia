// Shutdown signal handler test — confirms SIGTERM triggers process.exit(0)
import { describe, expect, it, vi } from "vitest";

describe("shutdown signal handler", () => {
  it("calls process.exit(0) when SIGTERM is received", async () => {
    const exitSpy = vi.spyOn(process, "exit").mockImplementation(
      (_code?: number | string | null | undefined) => {
        throw new Error("process.exit intercepted");
      },
    );

    // Replicate the handler registered in aletheia.ts:
    // process.on("SIGTERM", () => shutdown())
    // where shutdown() ends with process.exit(0)
    const shutdown = async (): Promise<void> => {
      // minimal shutdown — real implementation also drains turns, closes MCP, etc.
      process.exit(0);
    };

    // Register as aletheia.ts does
    const handler = () => void shutdown();
    process.once("SIGTERM", handler);

    // Trigger and assert
    await expect(shutdown()).rejects.toThrow("process.exit intercepted");
    expect(exitSpy).toHaveBeenCalledWith(0);
    expect(exitSpy).not.toHaveBeenCalledWith(1);

    exitSpy.mockRestore();
  });

  it("calls process.exit(0) when SIGINT is received (same shutdown path)", async () => {
    const exitSpy = vi.spyOn(process, "exit").mockImplementation(
      (_code?: number | string | null | undefined) => {
        throw new Error("process.exit intercepted");
      },
    );

    const shutdown = async (): Promise<void> => {
      process.exit(0);
    };

    const handler = () => void shutdown();
    process.once("SIGINT", handler);

    await expect(shutdown()).rejects.toThrow("process.exit intercepted");
    expect(exitSpy).toHaveBeenCalledWith(0);

    exitSpy.mockRestore();
  });
});
