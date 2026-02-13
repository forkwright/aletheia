import path from "node:path";
import { describe, expect, it, vi } from "vitest";
import { POSIX_ALETHEIA_TMP_DIR, resolvePreferredAletheiaTmpDir } from "./tmp-aletheia-dir.js";

describe("resolvePreferredAletheiaTmpDir", () => {
  it("prefers /tmp/aletheia when it already exists and is writable", () => {
    const accessSync = vi.fn();
    const statSync = vi.fn(() => ({ isDirectory: () => true }));
    const tmpdir = vi.fn(() => "/var/fallback");

    const resolved = resolvePreferredAletheiaTmpDir({ accessSync, statSync, tmpdir });

    expect(statSync).toHaveBeenCalledTimes(1);
    expect(accessSync).toHaveBeenCalledTimes(1);
    expect(resolved).toBe(POSIX_ALETHEIA_TMP_DIR);
    expect(tmpdir).not.toHaveBeenCalled();
  });

  it("prefers /tmp/aletheia when it does not exist but /tmp is writable", () => {
    const accessSync = vi.fn();
    const statSync = vi.fn(() => {
      const err = new Error("missing") as Error & { code?: string };
      err.code = "ENOENT";
      throw err;
    });
    const tmpdir = vi.fn(() => "/var/fallback");

    const resolved = resolvePreferredAletheiaTmpDir({ accessSync, statSync, tmpdir });

    expect(resolved).toBe(POSIX_ALETHEIA_TMP_DIR);
    expect(accessSync).toHaveBeenCalledWith("/tmp", expect.any(Number));
    expect(tmpdir).not.toHaveBeenCalled();
  });

  it("falls back to os.tmpdir()/aletheia when /tmp/aletheia is not a directory", () => {
    const accessSync = vi.fn();
    const statSync = vi.fn(() => ({ isDirectory: () => false }));
    const tmpdir = vi.fn(() => "/var/fallback");

    const resolved = resolvePreferredAletheiaTmpDir({ accessSync, statSync, tmpdir });

    expect(resolved).toBe(path.join("/var/fallback", "aletheia"));
    expect(tmpdir).toHaveBeenCalledTimes(1);
  });

  it("falls back to os.tmpdir()/aletheia when /tmp is not writable", () => {
    const accessSync = vi.fn((target: string) => {
      if (target === "/tmp") {
        throw new Error("read-only");
      }
    });
    const statSync = vi.fn(() => {
      const err = new Error("missing") as Error & { code?: string };
      err.code = "ENOENT";
      throw err;
    });
    const tmpdir = vi.fn(() => "/var/fallback");

    const resolved = resolvePreferredAletheiaTmpDir({ accessSync, statSync, tmpdir });

    expect(resolved).toBe(path.join("/var/fallback", "aletheia"));
    expect(tmpdir).toHaveBeenCalledTimes(1);
  });
});
