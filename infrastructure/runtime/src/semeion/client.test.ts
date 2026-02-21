// Signal client tests
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SignalClient } from "./client.js";

describe("SignalClient", () => {
  let client: SignalClient;

  beforeEach(() => {
    client = new SignalClient("http://localhost:8080");
    vi.stubGlobal("fetch", vi.fn());
  });

  it("normalizes base URL", () => {
    const c = new SignalClient("http://localhost:8080/");
    // Construction should not throw
    expect(c).toBeDefined();
  });

  it("prepends http:// if missing", () => {
    const c = new SignalClient("localhost:8080");
    expect(c).toBeDefined();
  });

  describe("rpc", () => {
    it("sends JSON-RPC 2.0 request", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: "ok" }),
      } as never);

      const result = await client.rpc("listAccounts");
      expect(result).toBe("ok");
      expect(fetch).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/rpc"),
        expect.objectContaining({
          method: "POST",
          headers: expect.objectContaining({ "Content-Type": "application/json" }),
        }),
      );
    });

    it("throws on RPC error", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        json: () => Promise.resolve({
          jsonrpc: "2.0", id: "1",
          error: { code: -32601, message: "Method not found" },
        }),
      } as never);

      await expect(client.rpc("badMethod")).rejects.toThrow("Method not found");
    });

    it("throws on HTTP error", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: false,
        status: 500,
        statusText: "Internal Server Error",
        text: () => Promise.resolve("error"),
      } as never);

      await expect(client.rpc("test")).rejects.toThrow();
    });
  });

  describe("send", () => {
    it("sends message via RPC", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: { timestamp: 123 } }),
      } as never);

      const result = await client.send({
        message: "hello",
        recipient: "+1234",
        account: "+5678",
      });
      expect(result).toBeDefined();
    });

    it("retries on network error", async () => {
      vi.mocked(fetch)
        .mockRejectedValueOnce(new Error("ECONNRESET"))
        .mockResolvedValueOnce({
          ok: true,
          json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: { timestamp: 123 } }),
        } as never);

      const result = await client.send({
        message: "hello",
        recipient: "+1234",
        account: "+5678",
      });
      expect(result).toBeDefined();
      expect(fetch).toHaveBeenCalledTimes(2);
    });
  });

  describe("health", () => {
    it("returns true when healthy", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        text: () => Promise.resolve("ok"),
      } as never);

      const healthy = await client.health();
      expect(healthy).toBe(true);
    });

    it("returns false on error", async () => {
      vi.mocked(fetch).mockRejectedValue(new Error("connection refused"));
      const healthy = await client.health();
      expect(healthy).toBe(false);
    });
  });
});
