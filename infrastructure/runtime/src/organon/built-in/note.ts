// Agent notes tool — explicit persistent memory that survives distillation
import type { ToolContext, ToolHandler } from "../registry.js";
import type { SessionStore } from "../../mneme/store.js";

const VALID_CATEGORIES = ["task", "decision", "preference", "correction", "context"] as const;
type NoteCategory = (typeof VALID_CATEGORIES)[number];
const MAX_NOTES_PER_SESSION = 50;
const MAX_NOTE_LENGTH = 500;

export function createNoteTool(store: SessionStore): ToolHandler {
  return {
    definition: {
      name: "note",
      description:
        "Write a note to your persistent session memory. Notes survive context distillation " +
        "and are automatically included in your system prompt. Use for important context " +
        "you don't want to lose across distillation boundaries.\n\n" +
        "USE WHEN:\n" +
        "- A significant decision is made and you want to remember why\n" +
        "- The user states a preference or correction\n" +
        "- You need to track task progress across potential distillations\n" +
        "- Context that's critical but might be lost in summarization\n\n" +
        "DO NOT USE WHEN:\n" +
        "- The information is already in your workspace files (MEMORY.md, etc.)\n" +
        "- It's a long-term fact better suited for mem0_search\n" +
        "- The session is short and distillation is unlikely\n\n" +
        "TIPS:\n" +
        "- Actions: 'add', 'list', 'delete'\n" +
        "- Categories: task, decision, preference, correction, context\n" +
        "- Notes are per-session, ordered by creation time\n" +
        "- Max 50 notes per session, 500 chars each\n" +
        "- Notes are injected into your system prompt automatically",
      input_schema: {
        type: "object",
        properties: {
          action: {
            type: "string",
            description: "Action: 'add', 'list', 'delete'",
          },
          content: {
            type: "string",
            description: "Note content (required for 'add', max 500 chars)",
          },
          category: {
            type: "string",
            description:
              "Note category: 'task' (progress tracking), 'decision' (choices made), " +
              "'preference' (user preferences), 'correction' (things that were wrong), " +
              "'context' (general important context). Default: 'context'",
          },
          id: {
            type: "number",
            description: "Note ID (required for 'delete')",
          },
        },
        required: ["action"],
      },
    },

    async execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const action = input["action"] as string;
      const sessionId = context.sessionId;
      const nousId = context.nousId;

      if (!sessionId || !nousId) {
        return "Error: note tool requires session and nous context.";
      }

      switch (action) {
        case "add": {
          const content = input["content"] as string | undefined;
          if (!content?.trim()) {
            return "Error: 'content' is required for 'add' action.";
          }

          const trimmed = content.trim().slice(0, MAX_NOTE_LENGTH);
          const rawCategory = (input["category"] as string) ?? "context";
          const category = VALID_CATEGORIES.includes(rawCategory as NoteCategory)
            ? (rawCategory as NoteCategory)
            : "context";

          // Check note limit
          const existing = store.getNotes(sessionId, { limit: MAX_NOTES_PER_SESSION + 1 });
          if (existing.length >= MAX_NOTES_PER_SESSION) {
            return `Error: Maximum ${MAX_NOTES_PER_SESSION} notes per session reached. Delete old notes first.`;
          }

          const id = store.addNote(sessionId, nousId, category, trimmed);
          return `Note #${id} saved (${category}): "${trimmed}"`;
        }

        case "list": {
          const filterCategory = input["category"] as string | undefined;
          const notes = store.getNotes(sessionId, {
            limit: 50,
            ...(filterCategory && VALID_CATEGORIES.includes(filterCategory as NoteCategory)
              ? { category: filterCategory as NoteCategory }
              : {}),
          });

          if (notes.length === 0) {
            return filterCategory
              ? `No notes with category '${filterCategory}'.`
              : "No notes in this session.";
          }

          const lines = notes.map(
            (n) => `#${n.id} [${n.category}] ${n.content} (${n.createdAt})`,
          );
          return `Notes (${notes.length}):\n${lines.join("\n")}`;
        }

        case "delete": {
          const noteId = input["id"] as number | undefined;
          if (noteId === undefined || noteId === null) {
            return "Error: 'id' is required for 'delete' action.";
          }
          const deleted = store.deleteNote(noteId, nousId);
          return deleted
            ? `Note #${noteId} deleted.`
            : `Note #${noteId} not found or not owned by you.`;
        }

        default:
          return `Unknown action '${action}'. Use 'add', 'list', or 'delete'.`;
      }
    },
  };
}
