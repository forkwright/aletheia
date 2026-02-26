import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// We test the module's exported behavior by mocking fetch
// and verifying refresh dedup, retry, and visibility-aware patterns.

describe("auth", () => {
  let auth: typeof import("./auth");

  beforeEach(async () => {
    vi.stubGlobal("fetch", vi.fn());
    // Provide minimal DOM stubs for visibility listener
    vi.stubGlobal("document", {
      addEventListener: vi.fn(),
      visibilityState: "visible",
    });
    vi.stubGlobal("window", {
      addEventListener: vi.fn(),
    });
    // Fresh module for each test (module-level state resets)
    vi.resetModules();
    auth = await import("./auth");
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("login sets access token and schedules refresh", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          accessToken: "tok-123",
          expiresIn: 900,
          username: "cody",
          role: "admin",
        }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      ),
    );

    const result = await auth.login("cody", "pass");
    expect(result.ok).toBe(true);
    expect(auth.getAccessToken()).toBe("tok-123");
  });

  it("refresh retries once on network error before failing", async () => {
    const mockFetch = vi.mocked(fetch);
    const failureHandler = vi.fn();
    auth.setAuthFailureHandler(failureHandler);

    // First attempt: network error
    mockFetch.mockRejectedValueOnce(new Error("Network error"));
    // Second attempt: also fails
    mockFetch.mockRejectedValueOnce(new Error("Network error"));

    const result = await auth.refresh();
    expect(result).toBe(false);
    expect(failureHandler).toHaveBeenCalledOnce();
    // 2 fetch calls = 1 original + 1 retry
    expect(mockFetch).toHaveBeenCalledTimes(2);
  });

  it("refresh retries once on server error then succeeds", async () => {
    const mockFetch = vi.mocked(fetch);

    // First attempt: 500
    mockFetch.mockResolvedValueOnce(
      new Response("Internal Server Error", { status: 500 }),
    );
    // Second attempt: success
    mockFetch.mockResolvedValueOnce(
      new Response(
        JSON.stringify({ accessToken: "tok-456", expiresIn: 900 }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      ),
    );

    const result = await auth.refresh();
    expect(result).toBe(true);
    expect(auth.getAccessToken()).toBe("tok-456");
    expect(mockFetch).toHaveBeenCalledTimes(2);
  });

  it("refresh does NOT retry on 401 (token truly invalid)", async () => {
    const mockFetch = vi.mocked(fetch);
    const failureHandler = vi.fn();
    auth.setAuthFailureHandler(failureHandler);

    mockFetch.mockResolvedValueOnce(
      new Response(JSON.stringify({ error: "Invalid refresh token" }), {
        status: 401,
      }),
    );

    const result = await auth.refresh();
    expect(result).toBe(false);
    expect(failureHandler).toHaveBeenCalledOnce();
    // Only 1 call — no retry on 401
    expect(mockFetch).toHaveBeenCalledTimes(1);
  });

  it("deduplicates concurrent refresh calls", async () => {
    const mockFetch = vi.mocked(fetch);

    // Single successful response — should only be called once
    mockFetch.mockResolvedValueOnce(
      new Response(
        JSON.stringify({ accessToken: "tok-dedup", expiresIn: 900 }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      ),
    );

    // Fire 3 concurrent refreshes
    const [r1, r2, r3] = await Promise.all([
      auth.refresh(),
      auth.refresh(),
      auth.refresh(),
    ]);

    expect(r1).toBe(true);
    expect(r2).toBe(true);
    expect(r3).toBe(true);
    // Only 1 actual fetch — the other 2 piggybacked
    expect(mockFetch).toHaveBeenCalledTimes(1);
    expect(auth.getAccessToken()).toBe("tok-dedup");
  });

  it("logout clears token", async () => {
    const mockFetch = vi.mocked(fetch);

    // Login first
    mockFetch.mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          accessToken: "tok-bye",
          expiresIn: 900,
          username: "cody",
          role: "admin",
        }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      ),
    );
    await auth.login("cody", "pass");
    expect(auth.getAccessToken()).toBe("tok-bye");

    // Logout
    mockFetch.mockResolvedValueOnce(new Response("{}", { status: 200 }));
    await auth.logout();
    expect(auth.getAccessToken()).toBeNull();
  });
});
