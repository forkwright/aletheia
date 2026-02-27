// Agora routing tests (Spec 34, Phase 4)
import { describe, expect, it } from "vitest";
import { parseTarget, resolveTarget, isRoutingError } from "./routing.js";
import { AgoraRegistry } from "./registry.js";

// ---------------------------------------------------------------------------
// parseTarget — pure parsing, no registry validation
// ---------------------------------------------------------------------------

describe("parseTarget", () => {
  describe("explicit channel prefix", () => {
    it("parses slack:C0123456789", () => {
      const result = parseTarget("slack:C0123456789");
      expect(result).toEqual({ channel: "slack", to: "C0123456789" });
    });

    it("parses slack:@username", () => {
      const result = parseTarget("slack:@username");
      expect(result).toEqual({ channel: "slack", to: "@username" });
    });

    it("parses slack:U0123456789", () => {
      const result = parseTarget("slack:U0123456789");
      expect(result).toEqual({ channel: "slack", to: "U0123456789" });
    });

    it("parses signal:+1234567890", () => {
      const result = parseTarget("signal:+1234567890");
      expect(result).toEqual({ channel: "signal", to: "+1234567890" });
    });

    it("parses discord:123456 (future channel)", () => {
      const result = parseTarget("discord:123456");
      expect(result).toEqual({ channel: "discord", to: "123456" });
    });

    it("parses matrix:@user:matrix.org (future channel)", () => {
      const result = parseTarget("matrix:@user:matrix.org");
      expect(result).toEqual({ channel: "matrix", to: "@user:matrix.org" });
    });

    it("is case-insensitive for channel prefix", () => {
      const result = parseTarget("Slack:C0123456789");
      expect(result).toEqual({ channel: "slack", to: "C0123456789" });
    });

    it("errors on empty address after prefix", () => {
      const result = parseTarget("slack:");
      expect(isRoutingError(result)).toBe(true);
      if (isRoutingError(result)) {
        expect(result.error).toContain("Missing address");
      }
    });
  });

  describe("legacy Signal formats (unqualified)", () => {
    it("parses +1234567890 as signal", () => {
      const result = parseTarget("+1234567890");
      expect(result).toEqual({ channel: "signal", to: "+1234567890" });
    });

    it("parses +15125551234 as signal", () => {
      const result = parseTarget("+15125551234");
      expect(result).toEqual({ channel: "signal", to: "+15125551234" });
    });

    it("parses group:ABCDEF as signal", () => {
      const result = parseTarget("group:ABCDEF");
      expect(result).toEqual({ channel: "signal", to: "group:ABCDEF" });
    });

    it("parses u:handle as signal", () => {
      const result = parseTarget("u:handle");
      expect(result).toEqual({ channel: "signal", to: "u:handle" });
    });

    it("parses group:base64/encoded+id== as signal", () => {
      const result = parseTarget("group:abc123/def+ghi==");
      expect(result).toEqual({ channel: "signal", to: "group:abc123/def+ghi==" });
    });
  });

  describe("edge cases", () => {
    it("errors on empty string", () => {
      const result = parseTarget("");
      expect(isRoutingError(result)).toBe(true);
      if (isRoutingError(result)) {
        expect(result.error).toContain("Empty target");
      }
    });

    it("errors on whitespace-only", () => {
      const result = parseTarget("   ");
      expect(isRoutingError(result)).toBe(true);
    });

    it("trims whitespace", () => {
      const result = parseTarget("  +1234567890  ");
      expect(result).toEqual({ channel: "signal", to: "+1234567890" });
    });

    it("errors on unknown format", () => {
      const result = parseTarget("randomtext");
      expect(isRoutingError(result)).toBe(true);
      if (isRoutingError(result)) {
        expect(result.error).toContain("Unknown target format");
      }
    });

    it("errors on unknown channel prefix", () => {
      const result = parseTarget("telegram:12345");
      expect(isRoutingError(result)).toBe(true);
      if (isRoutingError(result)) {
        expect(result.error).toContain("Unknown target format");
      }
    });

    it("does not treat short phone numbers as signal", () => {
      // +12345 is only 5 digits — too short
      const result = parseTarget("+12345");
      expect(isRoutingError(result)).toBe(true);
    });
  });
});

// ---------------------------------------------------------------------------
// resolveTarget — parsing + registry validation
// ---------------------------------------------------------------------------

describe("resolveTarget", () => {
  function makeRegistry(...channelIds: string[]): AgoraRegistry {
    const registry = new AgoraRegistry();
    for (const id of channelIds) {
      registry.register({
        id,
        name: id,
        capabilities: {
          threads: false, reactions: false, typing: false,
          media: false, streaming: false, richFormatting: false,
          maxTextLength: 4000,
        },
        start: async () => {},
        send: async () => ({ sent: true }),
        stop: async () => {},
      });
    }
    return registry;
  }

  it("resolves slack target when slack is registered", () => {
    const registry = makeRegistry("signal", "slack");
    const result = resolveTarget("slack:C0123", registry);
    expect(result).toEqual({ channel: "slack", to: "C0123" });
  });

  it("resolves signal phone when signal is registered", () => {
    const registry = makeRegistry("signal");
    const result = resolveTarget("+1234567890", registry);
    expect(result).toEqual({ channel: "signal", to: "+1234567890" });
  });

  it("errors when target channel is not registered", () => {
    const registry = makeRegistry("signal");
    const result = resolveTarget("slack:C0123", registry);
    expect(isRoutingError(result)).toBe(true);
    if (isRoutingError(result)) {
      expect(result.error).toContain("not configured");
      expect(result.error).toContain("signal");
    }
  });

  it("errors when no channels are registered", () => {
    const registry = new AgoraRegistry();
    const result = resolveTarget("slack:C0123", registry);
    expect(isRoutingError(result)).toBe(true);
    if (isRoutingError(result)) {
      expect(result.error).toContain("No channels configured");
    }
  });

  it("passes through parse errors", () => {
    const registry = makeRegistry("signal");
    const result = resolveTarget("", registry);
    expect(isRoutingError(result)).toBe(true);
    if (isRoutingError(result)) {
      expect(result.error).toContain("Empty target");
    }
  });
});
