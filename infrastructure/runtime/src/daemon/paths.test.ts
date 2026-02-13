import path from "node:path";
import { describe, expect, it } from "vitest";
import { resolveGatewayStateDir } from "./paths.js";

describe("resolveGatewayStateDir", () => {
  it("uses the default state dir when no overrides are set", () => {
    const env = { HOME: "/Users/test" };
    expect(resolveGatewayStateDir(env)).toBe(path.join("/Users/test", ".aletheia"));
  });

  it("appends the profile suffix when set", () => {
    const env = { HOME: "/Users/test", ALETHEIA_PROFILE: "rescue" };
    expect(resolveGatewayStateDir(env)).toBe(path.join("/Users/test", ".aletheia-rescue"));
  });

  it("treats default profiles as the base state dir", () => {
    const env = { HOME: "/Users/test", ALETHEIA_PROFILE: "Default" };
    expect(resolveGatewayStateDir(env)).toBe(path.join("/Users/test", ".aletheia"));
  });

  it("uses ALETHEIA_STATE_DIR when provided", () => {
    const env = { HOME: "/Users/test", ALETHEIA_STATE_DIR: "/var/lib/aletheia" };
    expect(resolveGatewayStateDir(env)).toBe(path.resolve("/var/lib/aletheia"));
  });

  it("expands ~ in ALETHEIA_STATE_DIR", () => {
    const env = { HOME: "/Users/test", ALETHEIA_STATE_DIR: "~/aletheia-state" };
    expect(resolveGatewayStateDir(env)).toBe(path.resolve("/Users/test/aletheia-state"));
  });

  it("preserves Windows absolute paths without HOME", () => {
    const env = { ALETHEIA_STATE_DIR: "C:\\State\\aletheia" };
    expect(resolveGatewayStateDir(env)).toBe("C:\\State\\aletheia");
  });
});
