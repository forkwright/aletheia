import { u as createLogger } from "./entry.mjs";
import { createHash } from "node:crypto";

//#region src/symbolon/audit-verify.ts
const log = createLogger("audit-verify");
function verifyAuditChain(db) {
	const rows = db.prepare("SELECT id, timestamp, actor, role, action, target, ip, status, checksum, previous_checksum FROM audit_log ORDER BY id ASC").all();
	if (rows.length === 0) return {
		valid: true,
		totalEntries: 0,
		checkedEntries: 0
	};
	let expectedPrevious = "GENESIS";
	let checked = 0;
	for (const row of rows) {
		const storedChecksum = row["checksum"];
		const storedPrevious = row["previous_checksum"];
		if (!storedChecksum) continue;
		if (storedPrevious !== expectedPrevious) {
			log.warn(`Chain break at entry #${row["id"]}: expected previous=${expectedPrevious}, got ${storedPrevious}`);
			return {
				valid: false,
				totalEntries: rows.length,
				checkedEntries: checked,
				firstEntry: rows[0]["timestamp"],
				lastEntry: rows[rows.length - 1]["timestamp"],
				tamperIndex: row["id"],
				tamperDetails: `Previous checksum mismatch at entry #${row["id"]}`
			};
		}
		const payload = [
			row["timestamp"],
			row["actor"],
			row["role"],
			row["action"],
			row["target"] ?? "",
			row["ip"],
			row["status"].toString(),
			storedPrevious
		].join("|");
		const computed = createHash("sha256").update(payload).digest("hex");
		if (computed !== storedChecksum) {
			log.warn(`Checksum mismatch at entry #${row["id"]}: computed=${computed}, stored=${storedChecksum}`);
			return {
				valid: false,
				totalEntries: rows.length,
				checkedEntries: checked,
				firstEntry: rows[0]["timestamp"],
				lastEntry: rows[rows.length - 1]["timestamp"],
				tamperIndex: row["id"],
				tamperDetails: `Checksum mismatch at entry #${row["id"]} — data may have been modified`
			};
		}
		expectedPrevious = storedChecksum;
		checked++;
	}
	return {
		valid: true,
		totalEntries: rows.length,
		checkedEntries: checked,
		firstEntry: rows[0]["timestamp"],
		lastEntry: rows[rows.length - 1]["timestamp"]
	};
}

//#endregion
export { verifyAuditChain };
//# sourceMappingURL=audit-verify-B2lpgrRC.mjs.map