// Agent import — restore an agent from an AgentFile export
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { createLogger } from "../koina/logger.js";
import type { AgentNote, DistillationPriming, SessionStore, WorkingState } from "../mneme/store.js";
import { paths } from "../taxis/paths.js";
import type { AgentFile, ExportedNote, ExportedSession } from "./export.js";

const log = createLogger("portability:import");

export interface ImportOptions {
  skipSessions?: boolean;
  skipWorkspace?: boolean;
  targetNousId?: string;
}

export interface ImportResult {
  nousId: string;
  filesRestored: number;
  sessionsImported: number;
  messagesImported: number;
  notesImported: number;
}

export async function importAgent(
  agentFile: AgentFile,
  store: SessionStore,
  opts?: ImportOptions,
): Promise<ImportResult> {
  if (agentFile.version !== 1) {
    throw new Error(`Unsupported agent file version: ${agentFile.version}`);
  }

  const nousId = opts?.targetNousId ?? agentFile.nous.id;
  const result: ImportResult = {
    nousId,
    filesRestored: 0,
    sessionsImported: 0,
    messagesImported: 0,
    notesImported: 0,
  };

  // Restore workspace files
  if (!opts?.skipWorkspace) {
    const workspaceDir = paths.nousDir(nousId);
    const files = agentFile.workspace.files;

    for (const [relPath, content] of Object.entries(files)) {
      const fullPath = join(workspaceDir, relPath);
      mkdirSync(dirname(fullPath), { recursive: true });
      writeFileSync(fullPath, content, "utf-8");
      result.filesRestored++;
    }

    if (result.filesRestored > 0) {
      log.info(`Restored ${result.filesRestored} workspace files for ${nousId}`);
    }
  }

  // Restore sessions
  if (!opts?.skipSessions) {
    for (const exportedSession of agentFile.sessions) {
      try {
        const imported = importSession(exportedSession, nousId, store);
        result.sessionsImported++;
        result.messagesImported += imported.messages;
        result.notesImported += imported.notes;
      } catch (err) {
        log.warn(`Failed to import session ${exportedSession.id}: ${err instanceof Error ? err.message : err}`);
      }
    }
  }

  if (agentFile.memory) {
    log.info(`Agent file contains memory data (vectors: ${agentFile.memory.vectors?.length ?? 0}, graph: ${agentFile.memory.graph ? "yes" : "no"}) — skipping (requires sidecar)`);
  }

  log.info(`Import complete for ${nousId}: ${result.filesRestored} files, ${result.sessionsImported} sessions, ${result.messagesImported} messages, ${result.notesImported} notes`);
  return result;
}

function importSession(
  exported: ExportedSession,
  nousId: string,
  store: SessionStore,
): { messages: number; notes: number } {
  const session = store.createSession(nousId, exported.sessionKey);
  let messages = 0;
  let notes = 0;

  // Import messages in sequence order
  const sortedMessages = [...exported.messages].sort((a, b) => a.seq - b.seq);

  for (const msg of sortedMessages) {
    store.appendMessage(session.id, msg.role as "user" | "assistant" | "tool_result", msg.content, {
      tokenEstimate: msg.tokenEstimate,
      isDistilled: msg.isDistilled,
    });
    messages++;
  }

  // Restore notes
  for (const note of exported.notes) {
    importNote(note, session.id, nousId, store);
    notes++;
  }

  // Restore working state
  if (exported.workingState) {
    store.updateWorkingState(session.id, exported.workingState as WorkingState);
  }

  // Restore distillation priming
  if (exported.distillationPriming) {
    store.setDistillationPriming(session.id, exported.distillationPriming as DistillationPriming);
  }

  log.debug(`Imported session ${exported.sessionKey} → ${session.id} (${messages} messages, ${notes} notes)`);
  return { messages, notes };
}

const VALID_NOTE_CATEGORIES: Set<string> = new Set(["task", "decision", "preference", "correction", "context"]);

function importNote(note: ExportedNote, sessionId: string, nousId: string, store: SessionStore): void {
  const category = VALID_NOTE_CATEGORIES.has(note.category)
    ? note.category as AgentNote["category"]
    : "context";
  store.addNote(sessionId, nousId, category, note.content);
}
