# Architecture Rules

Agent-action rules for module boundaries, event naming, and structural patterns in `infrastructure/runtime/src/`.

---

## Module Import Direction

Imports flow from higher layers to lower layers only. Never add an import that creates a cycle.

Compliant:
```typescript
// In nous/ (higher layer): importing lower-layer modules
import { ToolRegistry } from "../organon/registry.js";
import { createDefaultRouter } from "../hermeneus/router.js";
```

Non-compliant:
```typescript
// In koina/ (leaf node): importing any other module is forbidden
import { loadConfig } from "../taxis/loader.js";

// In taxis/: importing mneme, hermeneus, nous, etc. is forbidden
import { SessionStore } from "../mneme/store.js";
```

Key boundary rules (see docs/ARCHITECTURE.md#dependency-rules for the full table):

- `koina` imports nothing - it is the leaf node
- `taxis` imports only `koina`
- `symbolon` imports only `koina` - stateless auth utilities
- `daemon` imports `koina`, `taxis`, `mneme`, `hermeneus`, `nous`, `melete` - nothing imports daemon

Run `import/no-cycle` check after adding any new cross-module import.

See: docs/ARCHITECTURE.md#dependency-rules
See: docs/STANDARDS.md#rule-module-import-direction-layered-graph

---

## Event Names

All event bus events must use `noun:verb` format. Never use camelCase, hyphens, or freeform strings.

Compliant:
```typescript
eventBus.emit("turn:before", { sessionId, turnId });
eventBus.emit("tool:called", { name, sessionId });
eventBus.emit("session:created", { id, agentId });
eventBus.emit("distillation:complete", { sessionId });
```

Non-compliant:
```typescript
eventBus.emit("turnBefore", { sessionId });       // camelCase
eventBus.emit("tool-called", { name });            // hyphenated
eventBus.emit("sessionCreated", { id });           // camelCase, no colon
eventBus.emit("session_created", { id });          // underscore
```

Use the module name as the noun for module lifecycle events (e.g., `plugin:loaded`, `distillation:complete`).

See: docs/STANDARDS.md#rule-event-name-format-nounverb

---

## Logger Creation

Create loggers once at module scope. Never create loggers inside functions.

Compliant:
```typescript
// Module level — created once
const log = createLogger("nous:pipeline");
const log = createLogger("dianoia:orchestrator");
const log = createLogger("hermeneus");
```

Non-compliant:
```typescript
// Inside a function — recreated on every call
function handleTurn(turn: Turn) {
  const log = createLogger("nous");   // non-compliant
  log.info("handling turn");
}

const log = createLogger("myLogger");          // non-descriptive
const log = createLogger("src/nous/pipeline"); // path format, not module name
```

The module name must match the module's directory name, or use `"module:subcomponent"` for sub-components.

See: docs/STANDARDS.md#rule-logger-creation-pattern-createloggermodule-name

---

## Module Boundary Discipline

Import directly from the file that owns the symbol. Do not create `index.ts` barrel files that re-export a module's internals just to flatten import paths. Modules with a legitimate public API surface (e.g., `dianoia/index.ts` that defines the module's external interface) may use an `index.ts` to curate their exports.

Compliant:
```typescript
import { SessionStore } from "../mneme/store.js";
import { createLogger } from "../koina/logger.js";
import { AletheiaError } from "../koina/errors.js";

// Module with a curated public API surface — acceptable
import { orchestrate } from "../dianoia/index.js";
```

Non-compliant:
```typescript
// Gratuitous barrel that re-exports everything:
import { createLogger } from "../koina/index.js";  // loads all of koina

// Barrel created just to flatten paths:
import { SessionStore } from "../mneme/index.js";
```

See: docs/STANDARDS.md#rule-module-boundary-discipline

---

## Error Hierarchy

Extend the appropriate `AletheiaError` subclass. Never extend bare `Error`.

Compliant:
```typescript
// Check koina/errors.ts for existing subclasses first
throw new ToolError("execution failed", { code: "TOOL_EXEC_FAILED" });
throw new SessionError("session not found", { code: "SESSION_NOT_FOUND" });
throw new ValidationError("invalid input", { code: "VALIDATION_FAILED" });
```

Non-compliant:
```typescript
throw new Error("execution failed");

class MyError extends Error {  // extend AletheiaError subclass, not bare Error
  constructor(msg: string) { super(msg); }
}
```

Check `koina/errors.ts` for existing subclasses before creating a new error type.

See: docs/STANDARDS.md#rule-typed-errors-only
See: docs/ARCHITECTURE.md (koina public surface)

---

## Adding a New Module

When adding a new directory to `src/`:

1. Determine the module's layer position (what it imports, who may import it)
2. Add the module row to the Modules table in `docs/ARCHITECTURE.md`
3. Add the module's dependency rule row to the Dependency Rules table in `docs/ARCHITECTURE.md`
4. Wire into the initialization sequence in `aletheia.ts`
5. Run `import/no-cycle` check after adding cross-module imports

Agents dispatched in this codebase receive the updated `docs/ARCHITECTURE.md` boundary rules automatically once the file is updated.

See: docs/ARCHITECTURE.md#adding-a-module
