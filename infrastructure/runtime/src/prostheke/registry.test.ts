// Plugin registry tests
import { describe, expect, it, vi } from "vitest";
import { PluginRegistry } from "./registry.js";
import type { PluginApi, PluginDefinition } from "./types.js";

function makeConfig(): never {
  return { agents: { list: [] }, gateway: { port: 18789, auth: { mode: "none" } } } as never;
}

function makePlugin(id: string, hooks?: Partial<PluginDefinition["hooks"]>): PluginDefinition {
  return {
    manifest: { id, name: id, version: "1.0.0" },
    hooks: hooks as PluginDefinition["hooks"],
  };
}

describe("PluginRegistry", () => {
  it("registers and retrieves plugins", () => {
    const reg = new PluginRegistry(makeConfig());
    reg.register(makePlugin("mem0"));
    expect(reg.get("mem0")).toBeDefined();
    expect(reg.size).toBe(1);
  });

  it("deduplicates by id", () => {
    const reg = new PluginRegistry(makeConfig());
    reg.register(makePlugin("mem0"));
    reg.register(makePlugin("mem0"));
    expect(reg.size).toBe(1);
  });

  it("dispatches start hook", async () => {
    const onStart = vi.fn().mockResolvedValue(undefined);
    const reg = new PluginRegistry(makeConfig());
    reg.register(makePlugin("test", { onStart }));
    await reg.dispatchStart();
    expect(onStart).toHaveBeenCalled();
  });

  it("dispatches shutdown hook", async () => {
    const onShutdown = vi.fn().mockResolvedValue(undefined);
    const reg = new PluginRegistry(makeConfig());
    reg.register(makePlugin("test", { onShutdown }));
    await reg.dispatchShutdown();
    expect(onShutdown).toHaveBeenCalled();
  });

  it("catches errors in individual plugin hooks", async () => {
    const onStart = vi.fn().mockRejectedValue(new Error("plugin crash"));
    const reg = new PluginRegistry(makeConfig());
    reg.register(makePlugin("crashy", { onStart }));
    await expect(reg.dispatchStart()).resolves.toBeUndefined();
  });

  it("dispatches beforeTurn and afterTurn hooks", async () => {
    const onBeforeTurn = vi.fn().mockResolvedValue(undefined);
    const onAfterTurn = vi.fn().mockResolvedValue(undefined);
    const reg = new PluginRegistry(makeConfig());
    reg.register(makePlugin("test", { onBeforeTurn, onAfterTurn }));
    await reg.dispatchBeforeTurn({} as never);
    await reg.dispatchAfterTurn({} as never);
    expect(onBeforeTurn).toHaveBeenCalled();
    expect(onAfterTurn).toHaveBeenCalled();
  });

  it("dispatches distillation hooks", async () => {
    const onBeforeDistill = vi.fn().mockResolvedValue(undefined);
    const onAfterDistill = vi.fn().mockResolvedValue(undefined);
    const reg = new PluginRegistry(makeConfig());
    reg.register(makePlugin("test", { onBeforeDistill, onAfterDistill }));
    await reg.dispatchBeforeDistill({} as never);
    await reg.dispatchAfterDistill({} as never);
    expect(onBeforeDistill).toHaveBeenCalled();
    expect(onAfterDistill).toHaveBeenCalled();
  });

  it("dispatches configReload hook", async () => {
    const onConfigReload = vi.fn().mockResolvedValue(undefined);
    const reg = new PluginRegistry(makeConfig());
    reg.register(makePlugin("test", { onConfigReload }));
    await reg.dispatchConfigReload();
    expect(onConfigReload).toHaveBeenCalled();
  });

  it("passes PluginApi to hooks", async () => {
    let receivedApi: PluginApi | undefined;
    const onStart = vi.fn(async (api: PluginApi) => { receivedApi = api; });
    const reg = new PluginRegistry(makeConfig());
    reg.register(makePlugin("test", { onStart }));
    await reg.dispatchStart();
    expect(receivedApi).toBeDefined();
    expect(receivedApi!.config).toBeDefined();
    expect(typeof receivedApi!.log).toBe("function");
  });
});
