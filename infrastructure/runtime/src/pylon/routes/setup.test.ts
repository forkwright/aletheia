// Setup route tests
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("node:fs");
vi.mock("node:os", () => ({ homedir: () => "/home/testuser" }));

import * as fs from "node:fs";
import { setupRoutes } from "./setup.js";

function makeDeps(agentCount = 0) {
  return {
    config: { agents: { list: new Array(agentCount) } },
  } as never;
}

function makeRefs() {
  return {
    cron: () => null,
    watchdog: () => null,
    skills: () => null,
    mcp: () => null,
    commands: () => null,
  } as never;
}

function makeApp(agentCount = 0) {
  return setupRoutes(makeDeps(agentCount), makeRefs());
}

const SETUP_FLAG = "/home/testuser/.aletheia/.setup-complete";
const CRED_FILE = "/home/testuser/.aletheia/credentials/anthropic.json";
const CLAUDE_JSON = "/home/testuser/.claude.json";

beforeEach(() => {
  vi.mocked(fs.existsSync).mockReturnValue(false);
  vi.mocked(fs.readFileSync).mockImplementation(() => { throw new Error("ENOENT"); });
  vi.mocked(fs.writeFileSync).mockReturnValue(undefined);
  vi.mocked(fs.mkdirSync).mockReturnValue(undefined);
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("GET /api/setup/status", () => {
  it("returns false for both flags when nothing exists", async () => {
    const app = makeApp(0);
    const res = await app.request("/api/setup/status");
    expect(res.status).toBe(200);
    const body = await res.json() as Record<string, unknown>;
    expect(body.setupComplete).toBe(false);
    expect(body.credentialFound).toBe(false);
    expect(body.agentCount).toBe(0);
  });

  it("returns setupComplete true when flag file exists", async () => {
    vi.mocked(fs.existsSync).mockImplementation((p) => p === SETUP_FLAG);
    const app = makeApp(1);
    const res = await app.request("/api/setup/status");
    const body = await res.json() as Record<string, unknown>;
    expect(body.setupComplete).toBe(true);
    expect(body.agentCount).toBe(1);
  });

  it("returns credentialFound true when credential file exists", async () => {
    vi.mocked(fs.existsSync).mockImplementation((p) => p === CRED_FILE);
    const res = await makeApp().request("/api/setup/status");
    const body = await res.json() as Record<string, unknown>;
    expect(body.credentialFound).toBe(true);
    expect(body.setupComplete).toBe(false);
  });
});

describe("POST /api/setup/credentials", () => {
  it("returns 400 with no body and no ~/.claude.json", async () => {
    const res = await makeApp().request("/api/setup/credentials", { method: "POST" });
    expect(res.status).toBe(400);
    const body = await res.json() as Record<string, unknown>;
    expect(body.success).toBe(false);
    expect(typeof body.error).toBe("string");
  });

  it("auto-detects primaryApiKey from ~/.claude.json", async () => {
    vi.mocked(fs.readFileSync).mockImplementation((p) => {
      if (p === CLAUDE_JSON) return JSON.stringify({ primaryApiKey: "sk-ant-validkeyabcdefghij" });
      throw new Error("ENOENT");
    });
    const res = await makeApp().request("/api/setup/credentials", { method: "POST" });
    expect(res.status).toBe(200);
    const body = await res.json() as Record<string, unknown>;
    expect(body.success).toBe(true);
    expect(vi.mocked(fs.writeFileSync)).toHaveBeenCalledWith(
      CRED_FILE,
      expect.stringContaining("sk-ant-validkeyabcdefghij"),
      { mode: 0o600 },
    );
  });

  it("accepts manually provided API key in body", async () => {
    const res = await makeApp().request("/api/setup/credentials", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ apiKey: "sk-ant-manualkeyabcdefghij" }),
    });
    expect(res.status).toBe(200);
    const body = await res.json() as Record<string, unknown>;
    expect(body.success).toBe(true);
  });

  it("returns 400 for key missing sk-ant- prefix", async () => {
    const res = await makeApp().request("/api/setup/credentials", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ apiKey: "not-a-valid-key-at-all" }),
    });
    expect(res.status).toBe(400);
    const body = await res.json() as Record<string, unknown>;
    expect(body.success).toBe(false);
  });

  it("returns 400 for prefix-only key (sk-ant- with nothing after)", async () => {
    const res = await makeApp().request("/api/setup/credentials", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ apiKey: "sk-ant-" }),
    });
    expect(res.status).toBe(400);
    const body = await res.json() as Record<string, unknown>;
    expect(body.success).toBe(false);
  });

  it("detects OAuth-only Claude Code install and returns helpful error", async () => {
    vi.mocked(fs.readFileSync).mockImplementation((p) => {
      if (p === CLAUDE_JSON) return JSON.stringify({ oauthAccount: { email: "user@example.com" } });
      throw new Error("ENOENT");
    });
    const res = await makeApp().request("/api/setup/credentials", { method: "POST" });
    expect(res.status).toBe(400);
    const body = await res.json() as Record<string, unknown>;
    expect(body.success).toBe(false);
    expect(body.error as string).toContain("OAuth");
  });

  it("returns 500 when credential dir is not writable", async () => {
    vi.mocked(fs.mkdirSync).mockImplementation(() => { throw new Error("EACCES: permission denied"); });
    vi.mocked(fs.readFileSync).mockImplementation((p) => {
      if (p === CLAUDE_JSON) return JSON.stringify({ primaryApiKey: "sk-ant-validkeyabcdefghij" });
      throw new Error("ENOENT");
    });
    const res = await makeApp().request("/api/setup/credentials", { method: "POST" });
    expect(res.status).toBe(500);
    const body = await res.json() as Record<string, unknown>;
    expect(body.success).toBe(false);
    expect(typeof body.error).toBe("string");
  });

  it("preserves existing backupKeys when writing credentials", async () => {
    vi.mocked(fs.readFileSync).mockImplementation((p) => {
      if (p === CLAUDE_JSON) return JSON.stringify({ primaryApiKey: "sk-ant-newkeyabcdefghijklm" });
      if (p === CRED_FILE) return JSON.stringify({ backupKeys: ["old-backup"] });
      throw new Error("ENOENT");
    });
    vi.mocked(fs.existsSync).mockImplementation((p) => p === CRED_FILE);
    const res = await makeApp().request("/api/setup/credentials", { method: "POST" });
    expect(res.status).toBe(200);
    const written = vi.mocked(fs.writeFileSync).mock.calls[0]?.[1] as string;
    const parsed = JSON.parse(written) as Record<string, unknown>;
    expect(parsed.backupKeys).toEqual(["old-backup"]);
    expect(parsed.apiKey).toBe("sk-ant-newkeyabcdefghijklm");
  });
});

describe("POST /api/setup/complete", () => {
  it("writes flag file and returns success", async () => {
    const res = await makeApp().request("/api/setup/complete", { method: "POST" });
    expect(res.status).toBe(200);
    const body = await res.json() as Record<string, unknown>;
    expect(body.success).toBe(true);
    expect(vi.mocked(fs.writeFileSync)).toHaveBeenCalledWith(
      SETUP_FLAG,
      expect.any(String),
      "utf-8",
    );
  });

  it("returns 500 when flag file is not writable", async () => {
    vi.mocked(fs.writeFileSync).mockImplementation(() => { throw new Error("EACCES: permission denied"); });
    const res = await makeApp().request("/api/setup/complete", { method: "POST" });
    expect(res.status).toBe(500);
    const body = await res.json() as Record<string, unknown>;
    expect(body.success).toBe(false);
    expect(typeof body.error).toBe("string");
  });
});
