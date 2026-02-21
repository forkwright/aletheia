import { describe, expect, it } from "vitest";
import { type PiiScanConfig, scanText } from "./pii.js";

const mask: PiiScanConfig = { mode: "mask" };
const hash: PiiScanConfig = { mode: "hash" };
const warn: PiiScanConfig = { mode: "warn" };

describe("PII detection", () => {
  describe("phone detector", () => {
    it("detects US phone numbers", () => {
      const r = scanText("Call me at 555-123-4567 please", mask);
      expect(r.redacted).toBe(1);
      expect(r.matches[0]!.type).toBe("phone");
      expect(r.text).toContain("[REDACTED:phone]");
    });

    it("detects phone with parens", () => {
      const r = scanText("(555) 123-4567", mask);
      expect(r.redacted).toBe(1);
    });

    it("detects +1 prefix", () => {
      const r = scanText("Reach me at +1-555-123-4567", mask);
      expect(r.redacted).toBe(1);
    });

    it("does NOT match IPv4 addresses", () => {
      const r = scanText("Server at 192.168.0.29 is down", mask);
      expect(r.redacted).toBe(0);
    });

    it("does NOT match port numbers", () => {
      const r = scanText("Running on localhost:18789", mask);
      expect(r.redacted).toBe(0);
    });
  });

  describe("email detector", () => {
    it("detects email addresses", () => {
      const r = scanText("Email cody@example.com for info", mask);
      expect(r.redacted).toBe(1);
      expect(r.matches[0]!.type).toBe("email");
      expect(r.text).toContain("[REDACTED:email]");
    });

    it("detects complex email", () => {
      const r = scanText("user.name+tag@sub.domain.com", mask);
      expect(r.redacted).toBe(1);
    });
  });

  describe("SSN detector", () => {
    it("detects SSN with dashes", () => {
      const r = scanText("SSN: 123-45-6789", mask);
      expect(r.redacted).toBe(1);
      expect(r.matches[0]!.type).toBe("ssn");
    });

    it("detects SSN with spaces", () => {
      const r = scanText("SSN is 123 45 6789", mask);
      expect(r.redacted).toBe(1);
    });

    it("rejects invalid SSN starting with 000", () => {
      const r = scanText("000-12-3456", mask);
      expect(r.redacted).toBe(0);
    });

    it("rejects invalid SSN starting with 666", () => {
      const r = scanText("666-12-3456", mask);
      expect(r.redacted).toBe(0);
    });

    it("rejects SSN starting with 9xx", () => {
      const r = scanText("900-12-3456", mask);
      expect(r.redacted).toBe(0);
    });
  });

  describe("credit card detector", () => {
    it("detects valid Visa number", () => {
      const r = scanText("Card: 4111 1111 1111 1111", mask);
      expect(r.redacted).toBe(1);
      expect(r.matches[0]!.type).toBe("credit_card");
    });

    it("detects dashed format", () => {
      const r = scanText("4111-1111-1111-1111", mask);
      expect(r.redacted).toBe(1);
    });

    it("rejects invalid Luhn", () => {
      const r = scanText("4111 1111 1111 1112", mask);
      expect(r.redacted).toBe(0);
    });

    it("does NOT match UUIDs", () => {
      const r = scanText("id: 550e8400-e29b-41d4-a716-446655440000", mask);
      // UUIDs have hex chars, not just digits — CC regex won't match
      expect(r.matches.filter((m) => m.type === "credit_card")).toHaveLength(0);
    });
  });

  describe("API key detector", () => {
    it("detects Anthropic API keys", () => {
      const r = scanText("key: sk-ant-api03-abcdefghijklmnopqrstuvwxyz", mask);
      expect(r.redacted).toBe(1);
      expect(r.matches[0]!.type).toBe("api_key");
    });

    it("detects GitHub personal access tokens", () => {
      const r = scanText("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij", mask);
      expect(r.redacted).toBe(1);
    });

    it("detects JWTs", () => {
      const r = scanText(
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
        mask,
      );
      expect(r.matches.some((m) => m.type === "api_key")).toBe(true);
    });

    it("does NOT match hex-only git SHAs", () => {
      const r = scanText("commit 05cf8c914aa36d5f18e63bf01111908c52872106", mask);
      expect(r.matches.filter((m) => m.type === "api_key")).toHaveLength(0);
    });

    it("does NOT match short hex hashes", () => {
      const r = scanText("commit e2badc1", mask);
      expect(r.matches.filter((m) => m.type === "api_key")).toHaveLength(0);
    });
  });

  describe("address detector", () => {
    it("detects street address", () => {
      const r = scanText("Lives at 123 Main Street", mask);
      expect(r.redacted).toBe(1);
      expect(r.matches[0]!.type).toBe("address");
    });

    it("detects address with suite", () => {
      const r = scanText("Office: 456 Oak Ave, Suite 200", mask);
      expect(r.redacted).toBe(1);
    });

    it("detects abbreviated street types", () => {
      const r = scanText("1600 Pennsylvania Ave", mask);
      expect(r.redacted).toBe(1);
    });
  });
});

describe("redaction modes", () => {
  it("mask mode replaces with [REDACTED:type]", () => {
    const r = scanText("Email: test@example.com", mask);
    expect(r.text).toBe("Email: [REDACTED:email]");
    expect(r.redacted).toBe(1);
  });

  it("hash mode replaces with deterministic hash", () => {
    const r1 = scanText("Email: test@example.com", hash);
    const r2 = scanText("Email: test@example.com", hash);
    expect(r1.text).toBe(r2.text);
    expect(r1.text).toMatch(/\[EMAIL:[a-f0-9]{8}\]/);
    expect(r1.text).not.toContain("test@example.com");
  });

  it("hash mode produces different hashes for different values", () => {
    const r1 = scanText("a@b.com", hash);
    const r2 = scanText("x@y.com", hash);
    expect(r1.text).not.toBe(r2.text);
  });

  it("warn mode returns original text unchanged", () => {
    const r = scanText("Email: test@example.com", warn);
    expect(r.text).toBe("Email: test@example.com");
    expect(r.matches).toHaveLength(1);
    expect(r.redacted).toBe(0);
  });
});

describe("allowlist", () => {
  it("skips allowlisted exact email", () => {
    const r = scanText("Email: cody@example.com", {
      mode: "mask",
      allowlist: ["cody@example.com"],
    });
    expect(r.redacted).toBe(0);
  });

  it("supports wildcard patterns", () => {
    const r = scanText("Email: cody.kickertz@gmail.com", {
      mode: "mask",
      allowlist: ["cody.kickertz@*"],
    });
    expect(r.redacted).toBe(0);
  });

  it("still detects non-allowlisted items", () => {
    const r = scanText("Email: secret@evil.com and cody@safe.com", {
      mode: "mask",
      allowlist: ["cody@safe.com"],
    });
    expect(r.redacted).toBe(1);
    expect(r.text).toContain("[REDACTED:email]");
    expect(r.text).toContain("cody@safe.com");
  });
});

describe("multi-match handling", () => {
  it("redacts multiple PII types in one text", () => {
    const r = scanText("Call 555-123-4567 or email test@example.com", mask);
    expect(r.redacted).toBe(2);
    expect(r.text).toContain("[REDACTED:phone]");
    expect(r.text).toContain("[REDACTED:email]");
  });

  it("preserves surrounding text", () => {
    const r = scanText("Before 555-123-4567 after", mask);
    expect(r.text).toBe("Before [REDACTED:phone] after");
  });

  it("handles text with no PII", () => {
    const r = scanText("Just a normal message about code", mask);
    expect(r.redacted).toBe(0);
    expect(r.text).toBe("Just a normal message about code");
    expect(r.matches).toHaveLength(0);
  });
});

describe("detector selection", () => {
  it("only runs specified detectors", () => {
    const r = scanText("555-123-4567 test@example.com", {
      mode: "mask",
      detectors: ["email"],
    });
    expect(r.redacted).toBe(1);
    expect(r.matches[0]!.type).toBe("email");
    // Phone should still be in text
    expect(r.text).toContain("555-123-4567");
  });
});
