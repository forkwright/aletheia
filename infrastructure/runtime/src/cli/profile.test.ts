import path from "node:path";
import { describe, expect, it } from "vitest";
import { formatCliCommand } from "./command-format.js";
import { applyCliProfileEnv, parseCliProfileArgs } from "./profile.js";

describe("parseCliProfileArgs", () => {
  it("leaves gateway --dev for subcommands", () => {
    const res = parseCliProfileArgs([
      "node",
      "aletheia",
      "gateway",
      "--dev",
      "--allow-unconfigured",
    ]);
    if (!res.ok) {
      throw new Error(res.error);
    }
    expect(res.profile).toBeNull();
    expect(res.argv).toEqual(["node", "aletheia", "gateway", "--dev", "--allow-unconfigured"]);
  });

  it("still accepts global --dev before subcommand", () => {
    const res = parseCliProfileArgs(["node", "aletheia", "--dev", "gateway"]);
    if (!res.ok) {
      throw new Error(res.error);
    }
    expect(res.profile).toBe("dev");
    expect(res.argv).toEqual(["node", "aletheia", "gateway"]);
  });

  it("parses --profile value and strips it", () => {
    const res = parseCliProfileArgs(["node", "aletheia", "--profile", "work", "status"]);
    if (!res.ok) {
      throw new Error(res.error);
    }
    expect(res.profile).toBe("work");
    expect(res.argv).toEqual(["node", "aletheia", "status"]);
  });

  it("rejects missing profile value", () => {
    const res = parseCliProfileArgs(["node", "aletheia", "--profile"]);
    expect(res.ok).toBe(false);
  });

  it("rejects combining --dev with --profile (dev first)", () => {
    const res = parseCliProfileArgs(["node", "aletheia", "--dev", "--profile", "work", "status"]);
    expect(res.ok).toBe(false);
  });

  it("rejects combining --dev with --profile (profile first)", () => {
    const res = parseCliProfileArgs(["node", "aletheia", "--profile", "work", "--dev", "status"]);
    expect(res.ok).toBe(false);
  });
});

describe("applyCliProfileEnv", () => {
  it("fills env defaults for dev profile", () => {
    const env: Record<string, string | undefined> = {};
    applyCliProfileEnv({
      profile: "dev",
      env,
      homedir: () => "/home/peter",
    });
    const expectedStateDir = path.join(path.resolve("/home/peter"), ".aletheia-dev");
    expect(env.ALETHEIA_PROFILE).toBe("dev");
    expect(env.ALETHEIA_STATE_DIR).toBe(expectedStateDir);
    expect(env.ALETHEIA_CONFIG_PATH).toBe(path.join(expectedStateDir, "aletheia.json"));
    expect(env.ALETHEIA_GATEWAY_PORT).toBe("19001");
  });

  it("does not override explicit env values", () => {
    const env: Record<string, string | undefined> = {
      ALETHEIA_STATE_DIR: "/custom",
      ALETHEIA_GATEWAY_PORT: "19099",
    };
    applyCliProfileEnv({
      profile: "dev",
      env,
      homedir: () => "/home/peter",
    });
    expect(env.ALETHEIA_STATE_DIR).toBe("/custom");
    expect(env.ALETHEIA_GATEWAY_PORT).toBe("19099");
    expect(env.ALETHEIA_CONFIG_PATH).toBe(path.join("/custom", "aletheia.json"));
  });

  it("uses ALETHEIA_HOME when deriving profile state dir", () => {
    const env: Record<string, string | undefined> = {
      ALETHEIA_HOME: "/srv/aletheia-home",
      HOME: "/home/other",
    };
    applyCliProfileEnv({
      profile: "work",
      env,
      homedir: () => "/home/fallback",
    });

    const resolvedHome = path.resolve("/srv/aletheia-home");
    expect(env.ALETHEIA_STATE_DIR).toBe(path.join(resolvedHome, ".aletheia-work"));
    expect(env.ALETHEIA_CONFIG_PATH).toBe(
      path.join(resolvedHome, ".aletheia-work", "aletheia.json"),
    );
  });
});

describe("formatCliCommand", () => {
  it("returns command unchanged when no profile is set", () => {
    expect(formatCliCommand("aletheia doctor --fix", {})).toBe("aletheia doctor --fix");
  });

  it("returns command unchanged when profile is default", () => {
    expect(formatCliCommand("aletheia doctor --fix", { ALETHEIA_PROFILE: "default" })).toBe(
      "aletheia doctor --fix",
    );
  });

  it("returns command unchanged when profile is Default (case-insensitive)", () => {
    expect(formatCliCommand("aletheia doctor --fix", { ALETHEIA_PROFILE: "Default" })).toBe(
      "aletheia doctor --fix",
    );
  });

  it("returns command unchanged when profile is invalid", () => {
    expect(formatCliCommand("aletheia doctor --fix", { ALETHEIA_PROFILE: "bad profile" })).toBe(
      "aletheia doctor --fix",
    );
  });

  it("returns command unchanged when --profile is already present", () => {
    expect(
      formatCliCommand("aletheia --profile work doctor --fix", { ALETHEIA_PROFILE: "work" }),
    ).toBe("aletheia --profile work doctor --fix");
  });

  it("returns command unchanged when --dev is already present", () => {
    expect(formatCliCommand("aletheia --dev doctor", { ALETHEIA_PROFILE: "dev" })).toBe(
      "aletheia --dev doctor",
    );
  });

  it("inserts --profile flag when profile is set", () => {
    expect(formatCliCommand("aletheia doctor --fix", { ALETHEIA_PROFILE: "work" })).toBe(
      "aletheia --profile work doctor --fix",
    );
  });

  it("trims whitespace from profile", () => {
    expect(formatCliCommand("aletheia doctor --fix", { ALETHEIA_PROFILE: "  jbaletheia  " })).toBe(
      "aletheia --profile jbaletheia doctor --fix",
    );
  });

  it("handles command with no args after aletheia", () => {
    expect(formatCliCommand("aletheia", { ALETHEIA_PROFILE: "test" })).toBe(
      "aletheia --profile test",
    );
  });

  it("handles pnpm wrapper", () => {
    expect(formatCliCommand("pnpm aletheia doctor", { ALETHEIA_PROFILE: "work" })).toBe(
      "pnpm aletheia --profile work doctor",
    );
  });
});
