// Daemon retention cycle tests
import { describe, expect, it, vi } from "vitest";
import { runRetention } from "./retention.js";
import type { SessionStore } from "../mneme/store.js";
import type { PrivacySettings } from "../taxis/schema.js";

function makePrivacy(overrides?: Partial<PrivacySettings["retention"]>): PrivacySettings {
  return {
    retention: {
      distilledMessageMaxAgeDays: 90,
      archivedSessionMaxAgeDays: 180,
      toolResultMaxChars: 500,
      ...overrides,
    },
    hardenFilePermissions: true,
    pii: {},
  } as PrivacySettings;
}

function makeStore(returns?: Partial<Record<string, number>>) {
  return {
    purgeDistilledMessages: vi.fn().mockReturnValue(returns?.distilled ?? 3),
    purgeArchivedSessionMessages: vi.fn().mockReturnValue(returns?.archived ?? 5),
    truncateToolResults: vi.fn().mockReturnValue(returns?.truncated ?? 2),
    deleteEphemeralSessions: vi.fn().mockReturnValue(returns?.ephemeral ?? 1),
  } as unknown as SessionStore;
}

describe("runRetention", () => {
  it("calls all store methods with correct args", () => {
    const store = makeStore();
    const privacy = makePrivacy();
    runRetention(store, privacy);

    expect(store.purgeDistilledMessages).toHaveBeenCalledWith(90);
    expect(store.purgeArchivedSessionMessages).toHaveBeenCalledWith(180);
    expect(store.truncateToolResults).toHaveBeenCalledWith(500);
    expect(store.deleteEphemeralSessions).toHaveBeenCalled();
  });

  it("returns counts from store methods", () => {
    const store = makeStore({ distilled: 10, archived: 20, truncated: 5, ephemeral: 3 });
    const result = runRetention(store, makePrivacy());

    expect(result.distilledMessagesDeleted).toBe(10);
    expect(result.archivedMessagesDeleted).toBe(20);
    expect(result.toolResultsTruncated).toBe(5);
    expect(result.ephemeralSessionsDeleted).toBe(3);
  });

  it("isolates failures — one store method throwing does not block others", () => {
    const store = makeStore();
    (store.purgeDistilledMessages as ReturnType<typeof vi.fn>).mockImplementation(() => {
      throw new Error("disk full");
    });

    const result = runRetention(store, makePrivacy());

    expect(result.distilledMessagesDeleted).toBe(0);
    expect(result.archivedMessagesDeleted).toBe(5);
    expect(result.toolResultsTruncated).toBe(2);
    expect(result.ephemeralSessionsDeleted).toBe(1);
  });

  it("returns all zeros when store returns zeros", () => {
    const store = makeStore({ distilled: 0, archived: 0, truncated: 0, ephemeral: 0 });
    const result = runRetention(store, makePrivacy());

    expect(result.distilledMessagesDeleted).toBe(0);
    expect(result.archivedMessagesDeleted).toBe(0);
    expect(result.toolResultsTruncated).toBe(0);
    expect(result.ephemeralSessionsDeleted).toBe(0);
  });
});
