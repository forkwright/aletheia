import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { resolveAletheiaPackageRoot } from "./aletheia-root.js";
import {
  resolveControlUiDistIndexHealth,
  resolveControlUiDistIndexPath,
  resolveControlUiDistIndexPathForRoot,
  resolveControlUiRepoRoot,
  resolveControlUiRootOverrideSync,
  resolveControlUiRootSync,
} from "./control-ui-assets.js";

/** Try to create a symlink; returns false if the OS denies it (Windows CI without Developer Mode). */
async function trySymlink(target: string, linkPath: string): Promise<boolean> {
  try {
    await fs.symlink(target, linkPath);
    return true;
  } catch {
    return false;
  }
}

async function canonicalPath(p: string): Promise<string> {
  try {
    return await fs.realpath(p);
  } catch {
    return path.resolve(p);
  }
}

describe("control UI assets helpers", () => {
  it("resolves repo root from src argv1", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      await fs.mkdir(path.join(tmp, "ui"), { recursive: true });
      await fs.writeFile(path.join(tmp, "ui", "vite.config.ts"), "export {};\n");
      await fs.writeFile(path.join(tmp, "package.json"), "{}\n");
      await fs.mkdir(path.join(tmp, "src"), { recursive: true });
      await fs.writeFile(path.join(tmp, "src", "index.ts"), "export {};\n");

      expect(resolveControlUiRepoRoot(path.join(tmp, "src", "index.ts"))).toBe(tmp);
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });

  it("resolves repo root from dist argv1", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      await fs.mkdir(path.join(tmp, "ui"), { recursive: true });
      await fs.writeFile(path.join(tmp, "ui", "vite.config.ts"), "export {};\n");
      await fs.writeFile(path.join(tmp, "package.json"), "{}\n");
      await fs.mkdir(path.join(tmp, "dist"), { recursive: true });
      await fs.writeFile(path.join(tmp, "dist", "index.js"), "export {};\n");

      expect(resolveControlUiRepoRoot(path.join(tmp, "dist", "index.js"))).toBe(tmp);
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });

  it("resolves dist control-ui index path for dist argv1", async () => {
    const argv1 = path.resolve("/tmp", "pkg", "dist", "index.js");
    const distDir = path.dirname(argv1);
    expect(await resolveControlUiDistIndexPath(argv1)).toBe(
      path.join(distDir, "control-ui", "index.html"),
    );
  });

  it("resolves control-ui root for dist bundle argv1", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      await fs.mkdir(path.join(tmp, "dist", "control-ui"), { recursive: true });
      await fs.writeFile(path.join(tmp, "dist", "bundle.js"), "export {};\n");
      await fs.writeFile(path.join(tmp, "dist", "control-ui", "index.html"), "<html></html>\n");

      expect(resolveControlUiRootSync({ argv1: path.join(tmp, "dist", "bundle.js") })).toBe(
        path.join(tmp, "dist", "control-ui"),
      );
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });

  it("resolves control-ui root for dist/gateway bundle argv1", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      await fs.writeFile(path.join(tmp, "package.json"), JSON.stringify({ name: "aletheia" }));
      await fs.mkdir(path.join(tmp, "dist", "gateway"), { recursive: true });
      await fs.mkdir(path.join(tmp, "dist", "control-ui"), { recursive: true });
      await fs.writeFile(path.join(tmp, "dist", "gateway", "control-ui.js"), "export {};\n");
      await fs.writeFile(path.join(tmp, "dist", "control-ui", "index.html"), "<html></html>\n");

      expect(
        resolveControlUiRootSync({ argv1: path.join(tmp, "dist", "gateway", "control-ui.js") }),
      ).toBe(path.join(tmp, "dist", "control-ui"));
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });

  it("resolves control-ui root from override directory or index.html", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      const uiDir = path.join(tmp, "dist", "control-ui");
      await fs.mkdir(uiDir, { recursive: true });
      await fs.writeFile(path.join(uiDir, "index.html"), "<html></html>\n");

      expect(resolveControlUiRootOverrideSync(uiDir)).toBe(uiDir);
      expect(resolveControlUiRootOverrideSync(path.join(uiDir, "index.html"))).toBe(uiDir);
      expect(resolveControlUiRootOverrideSync(path.join(uiDir, "missing.html"))).toBeNull();
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });

  it("resolves dist control-ui index path from package root argv1", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      await fs.writeFile(path.join(tmp, "package.json"), JSON.stringify({ name: "aletheia" }));
      await fs.writeFile(path.join(tmp, "aletheia.mjs"), "export {};\n");
      await fs.mkdir(path.join(tmp, "dist", "control-ui"), { recursive: true });
      await fs.writeFile(path.join(tmp, "dist", "control-ui", "index.html"), "<html></html>\n");

      expect(await resolveControlUiDistIndexPath(path.join(tmp, "aletheia.mjs"))).toBe(
        path.join(tmp, "dist", "control-ui", "index.html"),
      );
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });

  it("resolves control-ui root for package entrypoint argv1", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      await fs.writeFile(path.join(tmp, "package.json"), JSON.stringify({ name: "aletheia" }));
      await fs.writeFile(path.join(tmp, "aletheia.mjs"), "export {};\n");
      await fs.mkdir(path.join(tmp, "dist", "control-ui"), { recursive: true });
      await fs.writeFile(path.join(tmp, "dist", "control-ui", "index.html"), "<html></html>\n");

      expect(resolveControlUiRootSync({ argv1: path.join(tmp, "aletheia.mjs") })).toBe(
        path.join(tmp, "dist", "control-ui"),
      );
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });

  it("resolves dist control-ui index path from .bin argv1", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      const binDir = path.join(tmp, "node_modules", ".bin");
      const pkgRoot = path.join(tmp, "node_modules", "aletheia");
      await fs.mkdir(binDir, { recursive: true });
      await fs.mkdir(path.join(pkgRoot, "dist", "control-ui"), { recursive: true });
      await fs.writeFile(path.join(binDir, "aletheia"), "#!/usr/bin/env node\n");
      await fs.writeFile(path.join(pkgRoot, "package.json"), JSON.stringify({ name: "aletheia" }));
      await fs.writeFile(path.join(pkgRoot, "dist", "control-ui", "index.html"), "<html></html>\n");

      expect(await resolveControlUiDistIndexPath(path.join(binDir, "aletheia"))).toBe(
        path.join(pkgRoot, "dist", "control-ui", "index.html"),
      );
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });

  it("resolves via fallback when package root resolution fails but package name matches", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      // Package named "aletheia" but resolveAletheiaPackageRoot failed for other reasons
      await fs.writeFile(path.join(tmp, "package.json"), JSON.stringify({ name: "aletheia" }));
      await fs.writeFile(path.join(tmp, "aletheia.mjs"), "export {};\n");
      await fs.mkdir(path.join(tmp, "dist", "control-ui"), { recursive: true });
      await fs.writeFile(path.join(tmp, "dist", "control-ui", "index.html"), "<html></html>\n");

      expect(await resolveControlUiDistIndexPath(path.join(tmp, "aletheia.mjs"))).toBe(
        path.join(tmp, "dist", "control-ui", "index.html"),
      );
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });

  it("returns null when package name does not match aletheia", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      // Package with different name should not be resolved
      await fs.writeFile(path.join(tmp, "package.json"), JSON.stringify({ name: "malicious-pkg" }));
      await fs.writeFile(path.join(tmp, "index.mjs"), "export {};\n");
      await fs.mkdir(path.join(tmp, "dist", "control-ui"), { recursive: true });
      await fs.writeFile(path.join(tmp, "dist", "control-ui", "index.html"), "<html></html>\n");

      expect(await resolveControlUiDistIndexPath(path.join(tmp, "index.mjs"))).toBeNull();
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });

  it("returns null when no control-ui assets exist", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      // Just a package.json, no dist/control-ui
      await fs.writeFile(path.join(tmp, "package.json"), JSON.stringify({ name: "some-pkg" }));
      await fs.writeFile(path.join(tmp, "index.mjs"), "export {};\n");

      expect(await resolveControlUiDistIndexPath(path.join(tmp, "index.mjs"))).toBeNull();
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });

  it("reports health for existing control-ui assets at a known root", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      const indexPath = resolveControlUiDistIndexPathForRoot(tmp);
      await fs.mkdir(path.dirname(indexPath), { recursive: true });
      await fs.writeFile(indexPath, "<html></html>\n");

      await expect(resolveControlUiDistIndexHealth({ root: tmp })).resolves.toEqual({
        indexPath,
        exists: true,
      });
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });

  it("reports health for missing control-ui assets at a known root", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      const indexPath = resolveControlUiDistIndexPathForRoot(tmp);
      await expect(resolveControlUiDistIndexHealth({ root: tmp })).resolves.toEqual({
        indexPath,
        exists: false,
      });
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });

  it("resolves control-ui root when argv1 is a symlink (nvm scenario)", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      const realPkg = path.join(tmp, "real-pkg");
      const bin = path.join(tmp, "bin");
      await fs.mkdir(realPkg, { recursive: true });
      await fs.mkdir(bin, { recursive: true });
      await fs.writeFile(path.join(realPkg, "package.json"), JSON.stringify({ name: "aletheia" }));
      await fs.writeFile(path.join(realPkg, "aletheia.mjs"), "export {};\n");
      await fs.mkdir(path.join(realPkg, "dist", "control-ui"), { recursive: true });
      await fs.writeFile(path.join(realPkg, "dist", "control-ui", "index.html"), "<html></html>\n");
      const ok = await trySymlink(
        path.join("..", "real-pkg", "aletheia.mjs"),
        path.join(bin, "aletheia"),
      );
      if (!ok) {
        return; // symlinks not supported (Windows CI)
      }

      const resolvedRoot = resolveControlUiRootSync({ argv1: path.join(bin, "aletheia") });
      expect(resolvedRoot).not.toBeNull();
      expect(await canonicalPath(resolvedRoot ?? "")).toBe(
        await canonicalPath(path.join(realPkg, "dist", "control-ui")),
      );
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });

  it("resolves package root via symlinked argv1", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      const realPkg = path.join(tmp, "real-pkg");
      const bin = path.join(tmp, "bin");
      await fs.mkdir(realPkg, { recursive: true });
      await fs.mkdir(bin, { recursive: true });
      await fs.writeFile(path.join(realPkg, "package.json"), JSON.stringify({ name: "aletheia" }));
      await fs.writeFile(path.join(realPkg, "aletheia.mjs"), "export {};\n");
      await fs.mkdir(path.join(realPkg, "dist", "control-ui"), { recursive: true });
      await fs.writeFile(path.join(realPkg, "dist", "control-ui", "index.html"), "<html></html>\n");
      const ok = await trySymlink(
        path.join("..", "real-pkg", "aletheia.mjs"),
        path.join(bin, "aletheia"),
      );
      if (!ok) {
        return; // symlinks not supported (Windows CI)
      }

      const packageRoot = await resolveAletheiaPackageRoot({ argv1: path.join(bin, "aletheia") });
      expect(packageRoot).not.toBeNull();
      expect(await canonicalPath(packageRoot ?? "")).toBe(await canonicalPath(realPkg));
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });

  it("resolves dist index path via symlinked argv1 (async)", async () => {
    const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-ui-"));
    try {
      const realPkg = path.join(tmp, "real-pkg");
      const bin = path.join(tmp, "bin");
      await fs.mkdir(realPkg, { recursive: true });
      await fs.mkdir(bin, { recursive: true });
      await fs.writeFile(path.join(realPkg, "package.json"), JSON.stringify({ name: "aletheia" }));
      await fs.writeFile(path.join(realPkg, "aletheia.mjs"), "export {};\n");
      await fs.mkdir(path.join(realPkg, "dist", "control-ui"), { recursive: true });
      await fs.writeFile(path.join(realPkg, "dist", "control-ui", "index.html"), "<html></html>\n");
      const ok = await trySymlink(
        path.join("..", "real-pkg", "aletheia.mjs"),
        path.join(bin, "aletheia"),
      );
      if (!ok) {
        return; // symlinks not supported (Windows CI)
      }

      const indexPath = await resolveControlUiDistIndexPath(path.join(bin, "aletheia"));
      expect(indexPath).not.toBeNull();
      expect(await canonicalPath(indexPath ?? "")).toBe(
        await canonicalPath(path.join(realPkg, "dist", "control-ui", "index.html")),
      );
    } finally {
      await fs.rm(tmp, { recursive: true, force: true });
    }
  });
});
