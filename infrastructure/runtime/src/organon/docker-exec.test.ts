import { beforeEach, describe, expect, it, vi } from "vitest";
import { type DockerExecOpts, dockerAvailable, execInDocker, resetDockerCheck } from "./docker-exec.js";

vi.mock("node:child_process", () => ({
  execSync: vi.fn(),
  exec: vi.fn(),
}));

vi.mock("node:util", () => ({
  promisify: (fn: unknown) => fn,
}));

const { execSync, exec } = await import("node:child_process");

const defaultConfig: DockerExecOpts["config"] = {
  enabled: true,
  mode: "docker",
  image: "aletheia-sandbox:latest",
  allowNetwork: false,
  mountWorkspace: "readonly",
  bypassFor: [],
  memoryLimit: "512m",
  cpuLimit: 1,
  denyPatterns: [],
  auditDenied: true,
};

describe("dockerAvailable", () => {
  beforeEach(() => {
    resetDockerCheck();
    vi.mocked(execSync).mockReset();
  });

  it("returns true when docker info succeeds", () => {
    vi.mocked(execSync).mockReturnValue(Buffer.from(""));
    expect(dockerAvailable()).toBe(true);
  });

  it("returns false when docker info throws", () => {
    vi.mocked(execSync).mockImplementation(() => { throw new Error("not found"); });
    expect(dockerAvailable()).toBe(false);
  });

  it("caches the result", () => {
    vi.mocked(execSync).mockReturnValue(Buffer.from(""));
    dockerAvailable();
    dockerAvailable();
    expect(execSync).toHaveBeenCalledTimes(1);
  });
});

describe("execInDocker", () => {
  beforeEach(() => {
    vi.mocked(exec).mockReset();
  });

  it("builds correct docker run command", async () => {
    vi.mocked(exec).mockImplementation((_cmd: unknown, _opts: unknown, cb?: unknown) => {
      if (typeof cb === "function") cb(null, { stdout: "ok", stderr: "" });
      return { stdout: "ok", stderr: "" } as ReturnType<typeof exec>;
    }) as unknown;

    // With promisify mocked to identity, exec returns directly
    vi.mocked(exec).mockResolvedValue({ stdout: "ok", stderr: "" } as never);

    const result = await execInDocker({
      command: "ls -la",
      workspace: "/mnt/ssd/aletheia/nous/syn",
      nousId: "main",
      timeout: 30000,
      config: defaultConfig,
    });

    expect(result.stdout).toBe("ok");
    const call = vi.mocked(exec).mock.calls[0]!;
    const cmd = call[0] as string;
    expect(cmd).toContain("docker run --rm");
    expect(cmd).toContain("--read-only");
    expect(cmd).toContain("--network none");
    expect(cmd).toContain("--memory 512m");
    expect(cmd).toContain("--cpus 1");
    expect(cmd).toContain("--user 1000:1000");
    expect(cmd).toContain(":ro");
    expect(cmd).toContain("aletheia-sandbox:latest");
    expect(cmd).toContain('sh -c "ls -la"');
  });

  it("allows network when configured", async () => {
    vi.mocked(exec).mockResolvedValue({ stdout: "", stderr: "" } as never);

    await execInDocker({
      command: "curl example.com",
      workspace: "/tmp",
      nousId: "test",
      timeout: 5000,
      config: { ...defaultConfig, allowNetwork: true },
    });

    const cmd = vi.mocked(exec).mock.calls[0]![0] as string;
    expect(cmd).not.toContain("--network none");
  });

  it("uses readwrite mount when configured", async () => {
    vi.mocked(exec).mockResolvedValue({ stdout: "", stderr: "" } as never);

    await execInDocker({
      command: "touch file",
      workspace: "/tmp",
      nousId: "test",
      timeout: 5000,
      config: { ...defaultConfig, mountWorkspace: "readwrite" },
    });

    const cmd = vi.mocked(exec).mock.calls[0]![0] as string;
    expect(cmd).toContain(":rw");
  });
});
