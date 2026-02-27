import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { isTokenExpired, readCredentials, refreshOAuthToken, proactiveRefresh, _resetRateLimit } from "./oauth-refresh.js";
import { readFileSync, writeFileSync } from "node:fs";

vi.mock("node:fs", () => ({
  readFileSync: vi.fn(),
  writeFileSync: vi.fn(),
}));

const mockReadFileSync = vi.mocked(readFileSync);
const mockWriteFileSync = vi.mocked(writeFileSync);

const validCreds = {
  type: "oauth",
  label: "test",
  token: "sk-ant-oat01-old-token",
  refreshToken: "sk-ant-ort01-refresh-token",
  expiresAt: Date.now() - 1000, // expired
  scopes: ["user:inference"],
  backupCredentials: [],
};

describe("isTokenExpired", () => {
  it("returns true when token is past expiry", () => {
    expect(isTokenExpired(Date.now() - 1000)).toBe(true);
  });

  it("returns true when token expires within 5 minute buffer", () => {
    expect(isTokenExpired(Date.now() + 2 * 60 * 1000)).toBe(true); // 2 min from now
  });

  it("returns false when token has plenty of time", () => {
    expect(isTokenExpired(Date.now() + 60 * 60 * 1000)).toBe(false); // 1 hour from now
  });
});

describe("readCredentials", () => {
  beforeEach(() => vi.resetAllMocks());

  it("returns parsed OAuth credentials", () => {
    mockReadFileSync.mockReturnValue(JSON.stringify(validCreds));
    const creds = readCredentials("/test/path");
    expect(creds).not.toBeNull();
    expect(creds!.type).toBe("oauth");
    expect(creds!.token).toBe("sk-ant-oat01-old-token");
  });

  it("returns null for non-OAuth credential files", () => {
    mockReadFileSync.mockReturnValue(JSON.stringify({ type: "api-key", apiKey: "sk-ant-..." }));
    expect(readCredentials("/test/path")).toBeNull();
  });

  it("returns null when file doesn't exist", () => {
    mockReadFileSync.mockImplementation(() => { throw new Error("ENOENT"); });
    expect(readCredentials("/test/path")).toBeNull();
  });

  it("returns null for invalid JSON", () => {
    mockReadFileSync.mockReturnValue("not json");
    expect(readCredentials("/test/path")).toBeNull();
  });
});

describe("refreshOAuthToken", () => {
  let originalFetch: typeof globalThis.fetch;

  beforeEach(() => {
    vi.resetAllMocks();
    _resetRateLimit();
    originalFetch = globalThis.fetch;
    mockReadFileSync.mockReturnValue(JSON.stringify(validCreds));
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
  });

  it("refreshes token successfully and writes updated credentials", async () => {
    const newToken = "sk-ant-oat01-new-token";
    const newRefresh = "sk-ant-ort01-new-refresh";

    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: () => Promise.resolve({
        access_token: newToken,
        refresh_token: newRefresh,
        expires_in: 43200, // 12h
      }),
    }) as unknown as typeof fetch;

    const result = await refreshOAuthToken("/test/path");

    expect(result.success).toBe(true);
    expect(result.newToken).toBe(newToken);
    expect(result.newExpiresAt).toBeGreaterThan(Date.now());

    // Verify credential file was written
    expect(mockWriteFileSync).toHaveBeenCalledOnce();
    const writtenData = JSON.parse(mockWriteFileSync.mock.calls[0]![1] as string);
    expect(writtenData.token).toBe(newToken);
    expect(writtenData.refreshToken).toBe(newRefresh);
    expect(writtenData.type).toBe("oauth");
    expect(writtenData.label).toBe("test"); // preserved
  });

  it("returns error when token endpoint returns 401", async () => {
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 401,
      statusText: "Unauthorized",
      text: () => Promise.resolve("invalid refresh token"),
    }) as unknown as typeof fetch;

    const result = await refreshOAuthToken("/test/path");
    expect(result.success).toBe(false);
    expect(result.error).toContain("Refresh token expired or invalid");
  });

  it("returns error when no refresh token in credentials", async () => {
    mockReadFileSync.mockReturnValue(JSON.stringify({ ...validCreds, refreshToken: "" }));

    const result = await refreshOAuthToken("/test/path");
    expect(result.success).toBe(false);
    expect(result.error).toContain("No refresh token");
  });

  it("returns error on network failure", async () => {
    globalThis.fetch = vi.fn().mockRejectedValue(new Error("ECONNREFUSED")) as unknown as typeof fetch;

    const result = await refreshOAuthToken("/test/path");
    expect(result.success).toBe(false);
    expect(result.error).toContain("Network error");
  });

  it("preserves existing credential fields on update", async () => {
    const credsWithExtras = {
      ...validCreds,
      subscriptionType: "max",
      backupCredentials: [{ type: "oauth", token: "backup" }],
    };
    mockReadFileSync.mockReturnValue(JSON.stringify(credsWithExtras));

    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: () => Promise.resolve({
        access_token: "new-token",
        expires_in: 43200,
      }),
    }) as unknown as typeof fetch;

    const result = await refreshOAuthToken("/test/path");
    expect(result.success).toBe(true);

    const written = JSON.parse(mockWriteFileSync.mock.calls[0]![1] as string);
    expect(written.subscriptionType).toBe("max");
    expect(written.backupCredentials).toEqual([{ type: "oauth", token: "backup" }]);
    // No new refresh token issued — original preserved
    expect(written.refreshToken).toBe(validCreds.refreshToken);
  });
});

describe("proactiveRefresh", () => {
  beforeEach(() => vi.resetAllMocks());

  it("returns false when token is still valid", async () => {
    const validTokenCreds = { ...validCreds, expiresAt: Date.now() + 60 * 60 * 1000 };
    mockReadFileSync.mockReturnValue(JSON.stringify(validTokenCreds));

    const result = await proactiveRefresh("/test/path");
    expect(result).toBe(false);
  });

  it("returns false when no credentials exist", async () => {
    mockReadFileSync.mockImplementation(() => { throw new Error("ENOENT"); });
    const result = await proactiveRefresh("/test/path");
    expect(result).toBe(false);
  });
});
