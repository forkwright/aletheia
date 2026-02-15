// Extended Signal client tests — sendTyping, sendReceipt, sendReaction, getAttachment, retry edge cases
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SignalClient } from "./client.js";

describe("SignalClient extended", () => {
  let client: SignalClient;

  beforeEach(() => {
    client = new SignalClient("http://localhost:8080");
    vi.stubGlobal("fetch", vi.fn());
  });

  describe("rpc", () => {
    it("returns undefined on 201 status", async () => {
      vi.mocked(fetch).mockResolvedValue({ status: 201 } as Response);
      const result = await client.rpc("send", {});
      expect(result).toBeUndefined();
    });
  });

  describe("send", () => {
    it("sends to group", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: {} }),
      } as never);

      await client.send({ message: "hello", groupId: "group123", account: "+1" });
      const body = JSON.parse(vi.mocked(fetch).mock.calls[0]![1]!.body as string);
      expect(body.params.groupId).toBe("group123");
    });

    it("sends with username", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: {} }),
      } as never);

      await client.send({ message: "hi", username: "user.01", account: "+1" });
      const body = JSON.parse(vi.mocked(fetch).mock.calls[0]![1]!.body as string);
      expect(body.params.username).toEqual(["user.01"]);
    });

    it("sends with attachments", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: {} }),
      } as never);

      await client.send({ message: "photo", recipient: "+1", attachments: ["/tmp/img.jpg"] });
      const body = JSON.parse(vi.mocked(fetch).mock.calls[0]![1]!.body as string);
      expect(body.params.attachments).toEqual(["/tmp/img.jpg"]);
    });

    it("sends with textStyle", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: {} }),
      } as never);

      await client.send({ message: "**bold**", recipient: "+1", textStyle: ["0:6:BOLD"] });
      const body = JSON.parse(vi.mocked(fetch).mock.calls[0]![1]!.body as string);
      expect(body.params["text-style"]).toEqual(["0:6:BOLD"]);
    });

    it("does not retry RPC errors (4xx-level)", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({
          jsonrpc: "2.0", id: "1",
          error: { code: -32601, message: "Unknown group" },
        }),
      } as never);

      await expect(client.send({ message: "hi", groupId: "bad" })).rejects.toThrow("Unknown group");
      expect(fetch).toHaveBeenCalledTimes(1);
    });

    it("exhausts retries on persistent network error", async () => {
      vi.mocked(fetch).mockRejectedValue(new Error("ECONNRESET"));

      await expect(client.send({ message: "hi", recipient: "+1" })).rejects.toThrow("ECONNRESET");
      // 1 initial + 2 retries = 3 total
      expect(fetch).toHaveBeenCalledTimes(3);
    });

    it("sends with null message (empty message)", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: {} }),
      } as never);

      await client.send({ recipient: "+1" });
      const body = JSON.parse(vi.mocked(fetch).mock.calls[0]![1]!.body as string);
      // message should not be in params when undefined
      expect(body.params.message).toBeUndefined();
    });
  });

  describe("sendTyping", () => {
    it("sends typing indicator to recipient", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: null }),
      } as never);

      await client.sendTyping({ recipient: "+1234", account: "+5678" });
      const body = JSON.parse(vi.mocked(fetch).mock.calls[0]![1]!.body as string);
      expect(body.method).toBe("sendTyping");
      expect(body.params.recipient).toBe("+1234");
    });

    it("sends stop typing indicator", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: null }),
      } as never);

      await client.sendTyping({ recipient: "+1234", stop: true });
      const body = JSON.parse(vi.mocked(fetch).mock.calls[0]![1]!.body as string);
      expect(body.params.stop).toBe(true);
    });

    it("sends typing to group", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: null }),
      } as never);

      await client.sendTyping({ groupId: "group123" });
      const body = JSON.parse(vi.mocked(fetch).mock.calls[0]![1]!.body as string);
      expect(body.params.groupId).toBe("group123");
    });
  });

  describe("sendReceipt", () => {
    it("sends read receipt", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: null }),
      } as never);

      await client.sendReceipt({ recipient: "+1234", targetTimestamp: 12345678 });
      const body = JSON.parse(vi.mocked(fetch).mock.calls[0]![1]!.body as string);
      expect(body.method).toBe("sendReceipt");
      expect(body.params.recipient).toBe("+1234");
      expect(body.params.type).toBe("read");
    });

    it("sends viewed receipt with account", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: null }),
      } as never);

      await client.sendReceipt({ recipient: "+1234", targetTimestamp: 12345678, type: "viewed", account: "+5678" });
      const body = JSON.parse(vi.mocked(fetch).mock.calls[0]![1]!.body as string);
      expect(body.params.type).toBe("viewed");
      expect(body.params.account).toBe("+5678");
    });
  });

  describe("sendReaction", () => {
    it("sends reaction to recipient", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: null }),
      } as never);

      await client.sendReaction({
        emoji: "👍",
        targetTimestamp: 12345678,
        targetAuthor: "+9999",
        recipient: "+1234",
      });
      const body = JSON.parse(vi.mocked(fetch).mock.calls[0]![1]!.body as string);
      expect(body.method).toBe("sendReaction");
      expect(body.params.emoji).toBe("👍");
      expect(body.params.recipients).toEqual(["+1234"]);
    });

    it("sends reaction to group", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: null }),
      } as never);

      await client.sendReaction({
        emoji: "❤️",
        targetTimestamp: 12345678,
        targetAuthor: "+9999",
        groupId: "group123",
      });
      const body = JSON.parse(vi.mocked(fetch).mock.calls[0]![1]!.body as string);
      expect(body.params.groupIds).toEqual(["group123"]);
    });

    it("sends reaction removal", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: null }),
      } as never);

      await client.sendReaction({
        emoji: "👍",
        targetTimestamp: 12345678,
        targetAuthor: "+9999",
        recipient: "+1234",
        remove: true,
      });
      const body = JSON.parse(vi.mocked(fetch).mock.calls[0]![1]!.body as string);
      expect(body.params.remove).toBe(true);
    });
  });

  describe("getAttachment", () => {
    it("fetches attachment by id", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: { path: "/tmp/att.jpg" } }),
      } as never);

      const result = await client.getAttachment({ id: "att_123" });
      expect(result).toEqual({ path: "/tmp/att.jpg" });
    });

    it("fetches attachment with account", async () => {
      vi.mocked(fetch).mockResolvedValue({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: { path: "/tmp/att.jpg" } }),
      } as never);

      await client.getAttachment({ id: "att_123", account: "+5678" });
      const body = JSON.parse(vi.mocked(fetch).mock.calls[0]![1]!.body as string);
      expect(body.params.account).toBe("+5678");
    });
  });
});
