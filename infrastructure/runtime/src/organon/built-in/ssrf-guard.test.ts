// SSRF guard tests
import { describe, expect, it, vi } from "vitest";

vi.mock("node:dns/promises", () => ({
  lookup: vi.fn(),
}));

import { validateUrl } from "./ssrf-guard.js";
import { lookup } from "node:dns/promises";

const mockLookup = vi.mocked(lookup);

describe("validateUrl", () => {
  it("blocks file: protocol", async () => {
    await expect(validateUrl("file:///etc/passwd")).rejects.toThrow("Blocked protocol");
  });

  it("blocks ftp: protocol", async () => {
    await expect(validateUrl("ftp://example.com")).rejects.toThrow("Blocked protocol");
  });

  it("blocks gopher: protocol", async () => {
    await expect(validateUrl("gopher://example.com")).rejects.toThrow("Blocked protocol");
  });

  it("blocks data: protocol", async () => {
    await expect(validateUrl("data:text/plain,hello")).rejects.toThrow("Blocked protocol");
  });

  it("blocks 127.x.x.x (loopback)", async () => {
    mockLookup.mockResolvedValue({ address: "127.0.0.1", family: 4 });
    await expect(validateUrl("http://localhost")).rejects.toThrow("private address");
  });

  it("blocks 10.x.x.x (private)", async () => {
    mockLookup.mockResolvedValue({ address: "10.0.0.1", family: 4 });
    await expect(validateUrl("http://internal.corp")).rejects.toThrow("private address");
  });

  it("blocks 192.168.x.x (private)", async () => {
    mockLookup.mockResolvedValue({ address: "192.168.1.1", family: 4 });
    await expect(validateUrl("http://router.local")).rejects.toThrow("private address");
  });

  it("blocks 172.16-31.x.x (private)", async () => {
    mockLookup.mockResolvedValue({ address: "172.16.0.1", family: 4 });
    await expect(validateUrl("http://internal")).rejects.toThrow("private address");
    mockLookup.mockResolvedValue({ address: "172.31.255.255", family: 4 });
    await expect(validateUrl("http://internal2")).rejects.toThrow("private address");
  });

  it("allows 172.32.x.x (not private)", async () => {
    mockLookup.mockResolvedValue({ address: "172.32.0.1", family: 4 });
    await expect(validateUrl("http://external.com")).resolves.toBeUndefined();
  });

  it("blocks ::1 (IPv6 loopback)", async () => {
    mockLookup.mockResolvedValue({ address: "::1", family: 6 });
    await expect(validateUrl("http://localhost6")).rejects.toThrow("private address");
  });

  it("blocks 0.0.0.0", async () => {
    mockLookup.mockResolvedValue({ address: "0.0.0.0", family: 4 });
    await expect(validateUrl("http://zero")).rejects.toThrow("private address");
  });

  it("blocks 169.254.x.x (link-local)", async () => {
    mockLookup.mockResolvedValue({ address: "169.254.1.1", family: 4 });
    await expect(validateUrl("http://link-local")).rejects.toThrow("private address");
  });

  it("allows public IPs", async () => {
    mockLookup.mockResolvedValue({ address: "93.184.216.34", family: 4 });
    await expect(validateUrl("http://example.com")).resolves.toBeUndefined();
  });
});
