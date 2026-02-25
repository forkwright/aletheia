// Audit trail chain verification
import { createHash } from "node:crypto";
import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";

const log = createLogger("audit-verify");

export interface VerifyResult {
  valid: boolean;
  totalEntries: number;
  checkedEntries: number;
  firstEntry?: string;
  lastEntry?: string;
  tamperIndex?: number;
  tamperDetails?: string;
}

export function verifyAuditChain(db: Database.Database): VerifyResult {
  const rows = db
    .prepare(
      "SELECT id, timestamp, actor, role, action, target, ip, status, checksum, previous_checksum FROM audit_log ORDER BY id ASC",
    )
    .all() as Array<Record<string, unknown>>;

  if (rows.length === 0) {
    return { valid: true, totalEntries: 0, checkedEntries: 0 };
  }

  let expectedPrevious = "GENESIS";
  let checked = 0;

  for (const row of rows) {
    const storedChecksum = row["checksum"] as string | null;
    const storedPrevious = row["previous_checksum"] as string | null;

    if (!storedChecksum) {
      continue;
    }

    if (storedPrevious !== expectedPrevious) {
      log.warn(`Chain break at entry #${row["id"]}: expected previous=${expectedPrevious}, got ${storedPrevious}`);
      return {
        valid: false,
        totalEntries: rows.length,
        checkedEntries: checked,
        firstEntry: rows[0]!["timestamp"] as string,
        lastEntry: rows[rows.length - 1]!["timestamp"] as string,
        tamperIndex: row["id"] as number,
        tamperDetails: `Previous checksum mismatch at entry #${row["id"]}`,
      };
    }

    const payload = [
      row["timestamp"] as string,
      row["actor"] as string,
      row["role"] as string,
      row["action"] as string,
      (row["target"] as string) ?? "",
      row["ip"] as string,
      (row["status"] as number).toString(),
      storedPrevious,
    ].join("|");
    const computed = createHash("sha256").update(payload).digest("hex");

    if (computed !== storedChecksum) {
      log.warn(`Checksum mismatch at entry #${row["id"]}: computed=${computed}, stored=${storedChecksum}`);
      return {
        valid: false,
        totalEntries: rows.length,
        checkedEntries: checked,
        firstEntry: rows[0]!["timestamp"] as string,
        lastEntry: rows[rows.length - 1]!["timestamp"] as string,
        tamperIndex: row["id"] as number,
        tamperDetails: `Checksum mismatch at entry #${row["id"]} — data may have been modified`,
      };
    }

    expectedPrevious = storedChecksum;
    checked++;
  }

  return {
    valid: true,
    totalEntries: rows.length,
    checkedEntries: checked,
    firstEntry: rows[0]!["timestamp"] as string,
    lastEntry: rows[rows.length - 1]!["timestamp"] as string,
  };
}
