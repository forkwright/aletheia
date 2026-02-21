// History stage tests
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RuntimeServices, TurnState } from "../types.js";

vi.mock("../../../hermeneus/token-counter.js", () => ({
  estimateTokens: vi.fn().mockReturnValue(10),
}));

vi.mock("../utils/build-messages.js", () => ({
  buildMessages: vi.fn().mockReturnValue([{ role: "user", content: "hello" }]),
}));

import { prepareHistory } from "./history.js";
import { buildMessages } from "../utils/build-messages.js";

function makeState(overrides?: Partial<TurnState>): TurnState {
  return {
    msg: { text: "hello" },
    nousId: "syl",
    sessionId: "ses_1",
    sessionKey: "main",
    nous: {},
    workspace: "/workspaces/syl",
    seq: 0,
    _historyBudget: 50000,
    ...overrides,
  } as unknown as TurnState;
}

function makeServices(): RuntimeServices {
  return {
    store: {
      getHistoryWithBudget: vi.fn().mockReturnValue([]),
      getUnsurfacedMessages: vi.fn().mockReturnValue([]),
      appendMessage: vi.fn().mockReturnValue(5),
      markMessagesSurfaced: vi.fn(),
    },
    plugins: null,
  } as unknown as RuntimeServices;
}

describe("prepareHistory", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("retrieves history and builds messages", async () => {
    const state = makeState();
    const services = makeServices();
    const result = await prepareHistory(state, services);

    expect(services.store.getHistoryWithBudget).toHaveBeenCalledWith("ses_1", 50000);
    expect(buildMessages).toHaveBeenCalled();
    expect(result.seq).toBe(5);
    expect(result.messages.length).toBeGreaterThan(0);
  });

  it("surfaces cross-agent messages", async () => {
    const state = makeState();
    const services = makeServices();
    (services.store.getUnsurfacedMessages as ReturnType<typeof vi.fn>).mockReturnValue([
      { id: 1, sourceNousId: "chiron", kind: "dispatch", content: "reminder: meeting at 3pm" },
    ]);

    await prepareHistory(state, services);

    expect(services.store.appendMessage).toHaveBeenCalledTimes(2); // cross-agent + user msg
    const firstCall = (services.store.appendMessage as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(firstCall![1]).toBe("user");
    expect(firstCall![2]).toContain("cross-agent messages");
    expect(services.store.markMessagesSurfaced).toHaveBeenCalledWith([1], "ses_1");
  });

  it("dispatches plugin beforeTurn when plugins available", async () => {
    const state = makeState();
    const services = makeServices();
    const dispatchBeforeTurn = vi.fn();
    (services as Record<string, unknown>).plugins = { dispatchBeforeTurn };

    await prepareHistory(state, services);

    expect(dispatchBeforeTurn).toHaveBeenCalledWith(expect.objectContaining({
      nousId: "syl",
      sessionId: "ses_1",
      messageText: "hello",
    }));
  });

  it("passes media to buildMessages when present", async () => {
    const media = [{ contentType: "image/png", data: "base64data" }];
    const state = makeState({ msg: { text: "look at this", media } } as Partial<TurnState>);
    const services = makeServices();

    await prepareHistory(state, services);

    const call = vi.mocked(buildMessages).mock.calls[0]!;
    expect(call[2]).toEqual(media);
  });
});
