import { describe, it, expect, vi, beforeEach } from "vitest";
import { getToken, setToken, clearToken, fetchAgents } from "./api";

// Mock localStorage
const localStorageMock = (() => {
  let store: Record<string, string> = {};
  return {
    getItem: vi.fn((key: string) => store[key] ?? null),
    setItem: vi.fn((key: string, value: string) => {
      store[key] = value;
    }),
    removeItem: vi.fn((key: string) => {
      delete store[key];
    }),
    clear: vi.fn(() => {
      store = {};
    }),
    get length() {
      return Object.keys(store).length;
    },
    key: vi.fn((_i: number) => null),
  };
})();

Object.defineProperty(globalThis, "localStorage", { value: localStorageMock });

// Mock import.meta.env.DEV
vi.stubGlobal("window", { location: { origin: "http://localhost:3000" } });

const mockFetch = vi.fn();
vi.stubGlobal("fetch", mockFetch);

beforeEach(() => {
  localStorageMock.clear();
  mockFetch.mockReset();
});

describe("token management", () => {
  it("getToken returns null when no token is set", () => {
    expect(getToken()).toBeNull();
  });

  it("setToken stores and getToken retrieves it", () => {
    setToken("test-token-123");
    expect(getToken()).toBe("test-token-123");
  });

  it("clearToken removes the token", () => {
    setToken("test-token-123");
    clearToken();
    expect(getToken()).toBeNull();
  });
});

describe("fetchAgents", () => {
  it("calls /api/agents and returns agents array", async () => {
    const agents = [{ id: "1", name: "Chiron" }];
    mockFetch.mockResolvedValueOnce({
      ok: true,
      status: 200,
      json: async () => ({ agents }),
    });

    const result = await fetchAgents();

    expect(mockFetch).toHaveBeenCalledOnce();
    const [url] = mockFetch.mock.calls[0];
    expect(url).toContain("/api/agents");
    expect(result).toEqual(agents);
  });

  it("throws on 401 response", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 401,
    });

    await expect(fetchAgents()).rejects.toThrow("Unauthorized");
  });

  it("throws with status and body on non-401 errors", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 500,
      text: async () => "Internal Server Error",
    });

    await expect(fetchAgents()).rejects.toThrow("API error 500: Internal Server Error");
  });
});
