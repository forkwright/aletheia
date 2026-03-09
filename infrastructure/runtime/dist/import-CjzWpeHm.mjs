import { c as paths, l as AletheiaError, u as createLogger } from "./entry.mjs";
import { dirname, join } from "node:path";
import { mkdirSync, writeFileSync } from "node:fs";

//#region src/portability/import.ts
const log = createLogger("portability:import");
async function importAgent(agentFile, store, opts) {
	if (agentFile.version !== 1) throw new AletheiaError({
		code: "PORTABILITY_IMPORT_FAILED",
		module: "portability",
		message: `Unsupported agent file version: ${agentFile.version}`,
		context: { version: agentFile.version }
	});
	const nousId = opts?.targetNousId ?? agentFile.nous.id;
	const result = {
		nousId,
		filesRestored: 0,
		sessionsImported: 0,
		messagesImported: 0,
		notesImported: 0
	};
	if (!opts?.skipWorkspace) {
		const workspaceDir = paths.nousDir(nousId);
		const files = agentFile.workspace.files;
		for (const [relPath, content] of Object.entries(files)) {
			const fullPath = join(workspaceDir, relPath);
			mkdirSync(dirname(fullPath), { recursive: true });
			writeFileSync(fullPath, content, "utf-8");
			result.filesRestored++;
		}
		if (result.filesRestored > 0) log.info(`Restored ${result.filesRestored} workspace files for ${nousId}`);
	}
	if (!opts?.skipSessions) for (const exportedSession of agentFile.sessions) try {
		const imported = importSession(exportedSession, nousId, store);
		result.sessionsImported++;
		result.messagesImported += imported.messages;
		result.notesImported += imported.notes;
	} catch (error) {
		log.warn(`Failed to import session ${exportedSession.id}: ${error instanceof Error ? error.message : error}`);
	}
	if (agentFile.memory) log.info(`Agent file contains memory data (vectors: ${agentFile.memory.vectors?.length ?? 0}, graph: ${agentFile.memory.graph ? "yes" : "no"}) — skipping (requires sidecar)`);
	log.info(`Import complete for ${nousId}: ${result.filesRestored} files, ${result.sessionsImported} sessions, ${result.messagesImported} messages, ${result.notesImported} notes`);
	return Promise.resolve(result);
}
function importSession(exported, nousId, store) {
	const session = store.createSession(nousId, exported.sessionKey);
	let messages = 0;
	let notes = 0;
	const sortedMessages = [...exported.messages].toSorted((a, b) => a.seq - b.seq);
	for (const msg of sortedMessages) {
		store.appendMessage(session.id, msg.role, msg.content, {
			tokenEstimate: msg.tokenEstimate,
			isDistilled: msg.isDistilled
		});
		messages++;
	}
	for (const note of exported.notes) {
		importNote(note, session.id, nousId, store);
		notes++;
	}
	if (exported.workingState) store.updateWorkingState(session.id, exported.workingState);
	if (exported.distillationPriming) store.setDistillationPriming(session.id, exported.distillationPriming);
	log.debug(`Imported session ${exported.sessionKey} → ${session.id} (${messages} messages, ${notes} notes)`);
	return {
		messages,
		notes
	};
}
const VALID_NOTE_CATEGORIES = new Set([
	"task",
	"decision",
	"preference",
	"correction",
	"context"
]);
function importNote(note, sessionId, nousId, store) {
	const category = VALID_NOTE_CATEGORIES.has(note.category) ? note.category : "context";
	store.addNote(sessionId, nousId, category, note.content);
}

//#endregion
export { importAgent };
//# sourceMappingURL=import-CjzWpeHm.mjs.map