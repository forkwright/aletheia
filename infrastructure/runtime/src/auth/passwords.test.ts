// Password hashing tests
import { describe, expect, it } from "vitest";
import { hashPassword, verifyPassword } from "./passwords.js";

describe("hashPassword", () => {
  it("produces scrypt format string", () => {
    const hash = hashPassword("test-password");
    expect(hash).toMatch(/^\$scrypt\$N=\d+,r=\d+,p=\d+\$/);
  });

  it("produces different hashes for same password (unique salt)", () => {
    const a = hashPassword("same");
    const b = hashPassword("same");
    expect(a).not.toBe(b);
  });
});

describe("verifyPassword", () => {
  it("verifies correct password", () => {
    const hash = hashPassword("correct-horse-battery-staple");
    expect(verifyPassword("correct-horse-battery-staple", hash)).toBe(true);
  });

  it("rejects wrong password", () => {
    const hash = hashPassword("correct");
    expect(verifyPassword("incorrect", hash)).toBe(false);
  });

  it("rejects malformed hash (wrong parts count)", () => {
    expect(verifyPassword("test", "not-a-hash")).toBe(false);
  });

  it("rejects malformed hash (wrong prefix)", () => {
    expect(verifyPassword("test", "$bcrypt$abc$def$ghi")).toBe(false);
  });

  it("handles empty password", () => {
    const hash = hashPassword("");
    expect(verifyPassword("", hash)).toBe(true);
    expect(verifyPassword("notempty", hash)).toBe(false);
  });
});
