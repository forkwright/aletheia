import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { describe, expect, it, vi } from "vitest";
import {
  resolveDefaultConfigCandidates,
  resolveConfigPath,
  resolveOAuthDir,
  resolveOAuthPath,
  resolveStateDir,
} from "./paths.js";

describe("oauth paths", () => {
  it("prefers ALETHEIA_OAUTH_DIR over ALETHEIA_STATE_DIR", () => {
    const env = {
      ALETHEIA_OAUTH_DIR: "/custom/oauth",
      ALETHEIA_STATE_DIR: "/custom/state",
    } as NodeJS.ProcessEnv;

    expect(resolveOAuthDir(env, "/custom/state")).toBe(path.resolve("/custom/oauth"));
    expect(resolveOAuthPath(env, "/custom/state")).toBe(
      path.join(path.resolve("/custom/oauth"), "oauth.json"),
    );
  });

  it("derives oauth path from ALETHEIA_STATE_DIR when unset", () => {
    const env = {
      ALETHEIA_STATE_DIR: "/custom/state",
    } as NodeJS.ProcessEnv;

    expect(resolveOAuthDir(env, "/custom/state")).toBe(path.join("/custom/state", "credentials"));
    expect(resolveOAuthPath(env, "/custom/state")).toBe(
      path.join("/custom/state", "credentials", "oauth.json"),
    );
  });
});

describe("state + config path candidates", () => {
  it("uses ALETHEIA_STATE_DIR when set", () => {
    const env = {
      ALETHEIA_STATE_DIR: "/new/state",
    } as NodeJS.ProcessEnv;

    expect(resolveStateDir(env, () => "/home/test")).toBe(path.resolve("/new/state"));
  });

  it("uses ALETHEIA_HOME for default state/config locations", () => {
    const env = {
      ALETHEIA_HOME: "/srv/aletheia-home",
    } as NodeJS.ProcessEnv;

    const resolvedHome = path.resolve("/srv/aletheia-home");
    expect(resolveStateDir(env)).toBe(path.join(resolvedHome, ".aletheia"));

    const candidates = resolveDefaultConfigCandidates(env);
    expect(candidates[0]).toBe(path.join(resolvedHome, ".aletheia", "aletheia.json"));
  });

  it("prefers ALETHEIA_HOME over HOME for default state/config locations", () => {
    const env = {
      ALETHEIA_HOME: "/srv/aletheia-home",
      HOME: "/home/other",
    } as NodeJS.ProcessEnv;

    const resolvedHome = path.resolve("/srv/aletheia-home");
    expect(resolveStateDir(env)).toBe(path.join(resolvedHome, ".aletheia"));

    const candidates = resolveDefaultConfigCandidates(env);
    expect(candidates[0]).toBe(path.join(resolvedHome, ".aletheia", "aletheia.json"));
  });

  it("orders default config candidates in a stable order", () => {
    const home = "/home/test";
    const resolvedHome = path.resolve(home);
    const candidates = resolveDefaultConfigCandidates({} as NodeJS.ProcessEnv, () => home);
    const expected = [
      path.join(resolvedHome, ".aletheia", "aletheia.json"),
      path.join(resolvedHome, ".aletheia", "openclaw.json"),
      path.join(resolvedHome, ".aletheia", "clawdbot.json"),
      path.join(resolvedHome, ".aletheia", "moltbot.json"),
      path.join(resolvedHome, ".aletheia", "moldbot.json"),
      path.join(resolvedHome, ".openclaw", "aletheia.json"),
      path.join(resolvedHome, ".openclaw", "openclaw.json"),
      path.join(resolvedHome, ".openclaw", "clawdbot.json"),
      path.join(resolvedHome, ".openclaw", "moltbot.json"),
      path.join(resolvedHome, ".openclaw", "moldbot.json"),
      path.join(resolvedHome, ".clawdbot", "aletheia.json"),
      path.join(resolvedHome, ".clawdbot", "openclaw.json"),
      path.join(resolvedHome, ".clawdbot", "clawdbot.json"),
      path.join(resolvedHome, ".clawdbot", "moltbot.json"),
      path.join(resolvedHome, ".clawdbot", "moldbot.json"),
      path.join(resolvedHome, ".moltbot", "aletheia.json"),
      path.join(resolvedHome, ".moltbot", "openclaw.json"),
      path.join(resolvedHome, ".moltbot", "clawdbot.json"),
      path.join(resolvedHome, ".moltbot", "moltbot.json"),
      path.join(resolvedHome, ".moltbot", "moldbot.json"),
      path.join(resolvedHome, ".moldbot", "aletheia.json"),
      path.join(resolvedHome, ".moldbot", "openclaw.json"),
      path.join(resolvedHome, ".moldbot", "clawdbot.json"),
      path.join(resolvedHome, ".moldbot", "moltbot.json"),
      path.join(resolvedHome, ".moldbot", "moldbot.json"),
    ];
    expect(candidates).toEqual(expected);
  });

  it("prefers ~/.aletheia when it exists and legacy dir is missing", async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-state-"));
    try {
      const newDir = path.join(root, ".aletheia");
      await fs.mkdir(newDir, { recursive: true });
      const resolved = resolveStateDir({} as NodeJS.ProcessEnv, () => root);
      expect(resolved).toBe(newDir);
    } finally {
      await fs.rm(root, { recursive: true, force: true });
    }
  });

  it("CONFIG_PATH prefers existing config when present", async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-config-"));
    const previousHome = process.env.HOME;
    const previousUserProfile = process.env.USERPROFILE;
    const previousHomeDrive = process.env.HOMEDRIVE;
    const previousHomePath = process.env.HOMEPATH;
    const previousAletheiaConfig = process.env.ALETHEIA_CONFIG_PATH;
    const previousAletheiaState = process.env.ALETHEIA_STATE_DIR;
    try {
      const legacyDir = path.join(root, ".aletheia");
      await fs.mkdir(legacyDir, { recursive: true });
      const legacyPath = path.join(legacyDir, "aletheia.json");
      await fs.writeFile(legacyPath, "{}", "utf-8");

      process.env.HOME = root;
      if (process.platform === "win32") {
        process.env.USERPROFILE = root;
        const parsed = path.win32.parse(root);
        process.env.HOMEDRIVE = parsed.root.replace(/\\$/, "");
        process.env.HOMEPATH = root.slice(parsed.root.length - 1);
      }
      delete process.env.ALETHEIA_CONFIG_PATH;
      delete process.env.ALETHEIA_STATE_DIR;

      vi.resetModules();
      const { CONFIG_PATH } = await import("./paths.js");
      expect(CONFIG_PATH).toBe(legacyPath);
    } finally {
      if (previousHome === undefined) {
        delete process.env.HOME;
      } else {
        process.env.HOME = previousHome;
      }
      if (previousUserProfile === undefined) {
        delete process.env.USERPROFILE;
      } else {
        process.env.USERPROFILE = previousUserProfile;
      }
      if (previousHomeDrive === undefined) {
        delete process.env.HOMEDRIVE;
      } else {
        process.env.HOMEDRIVE = previousHomeDrive;
      }
      if (previousHomePath === undefined) {
        delete process.env.HOMEPATH;
      } else {
        process.env.HOMEPATH = previousHomePath;
      }
      if (previousAletheiaConfig === undefined) {
        delete process.env.ALETHEIA_CONFIG_PATH;
      } else {
        process.env.ALETHEIA_CONFIG_PATH = previousAletheiaConfig;
      }
      if (previousAletheiaConfig === undefined) {
        delete process.env.ALETHEIA_CONFIG_PATH;
      } else {
        process.env.ALETHEIA_CONFIG_PATH = previousAletheiaConfig;
      }
      if (previousAletheiaState === undefined) {
        delete process.env.ALETHEIA_STATE_DIR;
      } else {
        process.env.ALETHEIA_STATE_DIR = previousAletheiaState;
      }
      if (previousAletheiaState === undefined) {
        delete process.env.ALETHEIA_STATE_DIR;
      } else {
        process.env.ALETHEIA_STATE_DIR = previousAletheiaState;
      }
      await fs.rm(root, { recursive: true, force: true });
      vi.resetModules();
    }
  });

  it("respects state dir overrides when config is missing", async () => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), "aletheia-config-override-"));
    try {
      const legacyDir = path.join(root, ".aletheia");
      await fs.mkdir(legacyDir, { recursive: true });
      const legacyConfig = path.join(legacyDir, "aletheia.json");
      await fs.writeFile(legacyConfig, "{}", "utf-8");

      const overrideDir = path.join(root, "override");
      const env = { ALETHEIA_STATE_DIR: overrideDir } as NodeJS.ProcessEnv;
      const resolved = resolveConfigPath(env, overrideDir, () => root);
      expect(resolved).toBe(path.join(overrideDir, "aletheia.json"));
    } finally {
      await fs.rm(root, { recursive: true, force: true });
    }
  });
});
