import { describe, expect, it } from "vitest";
import {
  buildParseArgv,
  getFlagValue,
  getCommandPath,
  getPrimaryCommand,
  getPositiveIntFlagValue,
  getVerboseFlag,
  hasHelpOrVersion,
  hasFlag,
  shouldMigrateState,
  shouldMigrateStateFromPath,
} from "./argv.js";

describe("argv helpers", () => {
  it("detects help/version flags", () => {
    expect(hasHelpOrVersion(["node", "aletheia", "--help"])).toBe(true);
    expect(hasHelpOrVersion(["node", "aletheia", "-V"])).toBe(true);
    expect(hasHelpOrVersion(["node", "aletheia", "status"])).toBe(false);
  });

  it("extracts command path ignoring flags and terminator", () => {
    expect(getCommandPath(["node", "aletheia", "status", "--json"], 2)).toEqual(["status"]);
    expect(getCommandPath(["node", "aletheia", "agents", "list"], 2)).toEqual(["agents", "list"]);
    expect(getCommandPath(["node", "aletheia", "status", "--", "ignored"], 2)).toEqual(["status"]);
  });

  it("returns primary command", () => {
    expect(getPrimaryCommand(["node", "aletheia", "agents", "list"])).toBe("agents");
    expect(getPrimaryCommand(["node", "aletheia"])).toBeNull();
  });

  it("parses boolean flags and ignores terminator", () => {
    expect(hasFlag(["node", "aletheia", "status", "--json"], "--json")).toBe(true);
    expect(hasFlag(["node", "aletheia", "--", "--json"], "--json")).toBe(false);
  });

  it("extracts flag values with equals and missing values", () => {
    expect(getFlagValue(["node", "aletheia", "status", "--timeout", "5000"], "--timeout")).toBe(
      "5000",
    );
    expect(getFlagValue(["node", "aletheia", "status", "--timeout=2500"], "--timeout")).toBe(
      "2500",
    );
    expect(getFlagValue(["node", "aletheia", "status", "--timeout"], "--timeout")).toBeNull();
    expect(getFlagValue(["node", "aletheia", "status", "--timeout", "--json"], "--timeout")).toBe(
      null,
    );
    expect(getFlagValue(["node", "aletheia", "--", "--timeout=99"], "--timeout")).toBeUndefined();
  });

  it("parses verbose flags", () => {
    expect(getVerboseFlag(["node", "aletheia", "status", "--verbose"])).toBe(true);
    expect(getVerboseFlag(["node", "aletheia", "status", "--debug"])).toBe(false);
    expect(getVerboseFlag(["node", "aletheia", "status", "--debug"], { includeDebug: true })).toBe(
      true,
    );
  });

  it("parses positive integer flag values", () => {
    expect(getPositiveIntFlagValue(["node", "aletheia", "status"], "--timeout")).toBeUndefined();
    expect(
      getPositiveIntFlagValue(["node", "aletheia", "status", "--timeout"], "--timeout"),
    ).toBeNull();
    expect(
      getPositiveIntFlagValue(["node", "aletheia", "status", "--timeout", "5000"], "--timeout"),
    ).toBe(5000);
    expect(
      getPositiveIntFlagValue(["node", "aletheia", "status", "--timeout", "nope"], "--timeout"),
    ).toBeUndefined();
  });

  it("builds parse argv from raw args", () => {
    const nodeArgv = buildParseArgv({
      programName: "aletheia",
      rawArgs: ["node", "aletheia", "status"],
    });
    expect(nodeArgv).toEqual(["node", "aletheia", "status"]);

    const versionedNodeArgv = buildParseArgv({
      programName: "aletheia",
      rawArgs: ["node-22", "aletheia", "status"],
    });
    expect(versionedNodeArgv).toEqual(["node-22", "aletheia", "status"]);

    const versionedNodeWindowsArgv = buildParseArgv({
      programName: "aletheia",
      rawArgs: ["node-22.2.0.exe", "aletheia", "status"],
    });
    expect(versionedNodeWindowsArgv).toEqual(["node-22.2.0.exe", "aletheia", "status"]);

    const versionedNodePatchlessArgv = buildParseArgv({
      programName: "aletheia",
      rawArgs: ["node-22.2", "aletheia", "status"],
    });
    expect(versionedNodePatchlessArgv).toEqual(["node-22.2", "aletheia", "status"]);

    const versionedNodeWindowsPatchlessArgv = buildParseArgv({
      programName: "aletheia",
      rawArgs: ["node-22.2.exe", "aletheia", "status"],
    });
    expect(versionedNodeWindowsPatchlessArgv).toEqual(["node-22.2.exe", "aletheia", "status"]);

    const versionedNodeWithPathArgv = buildParseArgv({
      programName: "aletheia",
      rawArgs: ["/usr/bin/node-22.2.0", "aletheia", "status"],
    });
    expect(versionedNodeWithPathArgv).toEqual(["/usr/bin/node-22.2.0", "aletheia", "status"]);

    const nodejsArgv = buildParseArgv({
      programName: "aletheia",
      rawArgs: ["nodejs", "aletheia", "status"],
    });
    expect(nodejsArgv).toEqual(["nodejs", "aletheia", "status"]);

    const nonVersionedNodeArgv = buildParseArgv({
      programName: "aletheia",
      rawArgs: ["node-dev", "aletheia", "status"],
    });
    expect(nonVersionedNodeArgv).toEqual(["node", "aletheia", "node-dev", "aletheia", "status"]);

    const directArgv = buildParseArgv({
      programName: "aletheia",
      rawArgs: ["aletheia", "status"],
    });
    expect(directArgv).toEqual(["node", "aletheia", "status"]);

    const bunArgv = buildParseArgv({
      programName: "aletheia",
      rawArgs: ["bun", "src/entry.ts", "status"],
    });
    expect(bunArgv).toEqual(["bun", "src/entry.ts", "status"]);
  });

  it("builds parse argv from fallback args", () => {
    const fallbackArgv = buildParseArgv({
      programName: "aletheia",
      fallbackArgv: ["status"],
    });
    expect(fallbackArgv).toEqual(["node", "aletheia", "status"]);
  });

  it("decides when to migrate state", () => {
    expect(shouldMigrateState(["node", "aletheia", "status"])).toBe(false);
    expect(shouldMigrateState(["node", "aletheia", "health"])).toBe(false);
    expect(shouldMigrateState(["node", "aletheia", "sessions"])).toBe(false);
    expect(shouldMigrateState(["node", "aletheia", "memory", "status"])).toBe(false);
    expect(shouldMigrateState(["node", "aletheia", "agent", "--message", "hi"])).toBe(false);
    expect(shouldMigrateState(["node", "aletheia", "agents", "list"])).toBe(true);
    expect(shouldMigrateState(["node", "aletheia", "message", "send"])).toBe(true);
  });

  it("reuses command path for migrate state decisions", () => {
    expect(shouldMigrateStateFromPath(["status"])).toBe(false);
    expect(shouldMigrateStateFromPath(["agents", "list"])).toBe(true);
  });
});
