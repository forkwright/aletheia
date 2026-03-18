# Aletheia: lexicon

*Living registry. Updated as crates are added or renamed.*
*For the naming methodology and construction system, see [gnomon.md](gnomon.md).*

---

## Project name

**Aletheia** (ἀλήθεια): Truth as unconcealment, the negation of forgetting.

| Layer | Reading |
|-------|---------|
| L1 | Multi-agent cognitive runtime: memory, orchestration, multi-nous cognition |
| L2 | The substrate; everything else is housed within it |
| L3 | Truth as unconcealment, ἀ-λήθεια: not-hidden. Not truth as correspondence but truth as *revealing what was hidden* |
| L4 | The system itself practices unconcealment, surfacing latent knowledge, refusing to let things stay forgotten |

---

## Crate names

### Leaf layer

| Crate | Greek | Over | L3 Essential Nature |
|-------|-------|------|---------------------|
| **Koina** | κοινά | "utils" | The commons, what is held in common. Errors (snafu), tracing, fs utilities, safe wrappers. The public utility of collective thought. |
| **Symbolon** | σύμβολον | "auth" | The token of recognition: two halves of a broken coin. JWT tokens, password hashing, RBAC policies. Identity as shared history. |

### Low layer

| Crate | Greek | Over | L3 Essential Nature |
|-------|-------|------|---------------------|
| **Taxis** | τάξις | "config" | The arrangement that makes a collection coherent, Aristotle's word for ordered structure. Config loading (figment TOML cascade), path resolution, oikos hierarchy. |
| **Hermeneus** | ἑρμηνεύς | "provider" | Hermes' art of carrying meaning across worlds. Anthropic client, model routing, credential management. The translation layer between nous and LLM backends. |
| **Mneme** | μνήμη | "memory" | Memory as active faculty, the Muse's gift, not passive storage. Facade re-exporting eidos, krites, graphe, episteme. |
| **Eidos** | εἶδος | "types" | The visible form: what makes a Fact a Fact, a Session a Session. Shared memory types, IDs, error definitions. |
| **Krites** | κριτής | "engine" | The judge: evaluates queries, decides what follows from rules and facts. Vendored CozoDB datalog engine. |
| **Graphe** | γραφή | "session" | The inscription: messages written, turns recorded, history preserved. SQLite session persistence. |
| **Episteme** | ἐπιστήμη | "knowledge" | Systematic knowledge: extraction, recall, scoring, dedup, succession. Transforms raw experience into understanding. |
| **Organon** | ὄργανον | "tools" | Aristotle's name for the instruments of thought, that by which the mind extends itself. Tool registry, definitions, built-in tool set. |
| **Agora** | ἀγορά | "channels" | The gathering place, Greek civic space where voices meet and meaning is made public. Channel registry, ChannelProvider trait, Signal JSON-RPC client. |
| **Melete** | μελέτη | "distillation" | Disciplined practice, one of the original Muses. Context distillation, compression strategies, token budget management. Attending carefully to what was. |

### Mid layer

| Crate | Greek | Over | L3 Essential Nature |
|-------|-------|------|---------------------|
| **Nous** | νοῦς | "agent" | Direct apprehension, the highest mode of knowing, distinct from discursive thought. The agent pipeline: bootstrap, recall, execute, finalize. |
| **Dianoia** | διάνοια | "planning" | Discursive reasoning, Plato's divided line: thinking *through* problems step by step. Multi-phase planning orchestrator, project context tracking. |
| **Thesauros** | θησαυρός | "packs" | The storehouse, a treasury of accumulated knowledge held ready for use. Domain pack loader: knowledge, tools, config overlays. |
| **Dokimion** | δοκίμιον | "eval" | The test, the proof, that which demonstrates whether something is genuine. Behavioral evaluation framework, HTTP scenario runner. |

### High layer

| Crate | Greek | Over | L3 Essential Nature |
|-------|-------|------|---------------------|
| **Pylon** | πυλών | "gateway" | The gate, the architectural boundary between inside and outside. Axum HTTP gateway, SSE streaming, auth middleware. |

### Top layer

| Crate | Greek | Over | L3 Essential Nature |
|-------|-------|------|---------------------|
| **Aletheia** | ἀλήθεια | "binary" | The entrypoint. The system as a whole, invoked by name. CLI, serve mode, the single binary. |

### Internal modules

| Name | Greek | Over | L3 Essential Nature |
|------|-------|------|---------------------|
| **Semeion** | σημεῖον | "signal" | The sign, the mark; communication as semiotics. Signal messaging integration inside Agora. |
| **Oikonomos** | οἰκονόμος | "daemon" | The household steward, managing the oikos and keeping things in order. Background scheduling, cron, lifecycle events. |
| **Prosoche** | προσοχή | "heartbeat" | Sustained directed attention, the practice that makes unconcealment possible. Attention checks, health monitoring, directive surfacing. |
| **Theke** | θήκη | "vault" | A repository in the original sense, a place that holds what matters. Tier-0 instance directory, human + nous collaborative space. |
| **Diaporeia** | διαπορεία | "mcp-server" | Passage through, transit between worlds. MCP server bridge for external AI agents. |

### Planned

| Name | Greek | Over | L3 Essential Nature |
|------|-------|------|---------------------|
| **Prostheke** | προσθήκη | "plugins" | The supplement, the addition. WASM plugin host (wasmtime): extend the system without touching core. |
| **Autarkeia** | αὐτάρκεια | "export/import" | Self-sufficiency, the Stoic virtue of needing nothing external. Agent portability across instances. |
| **Theatron** | θέατρον | "frontend" | The place for seeing, Greek theater where truth was made visible. TUI client and future UI surfaces. |

---

## Key topological relationships

- **Nous ↔ Dianoia**: Plato's divided line. Noesis (immediate apprehension) and dianoia (step-by-step reasoning). Both modes are necessary.
- **Prosoche → Aletheia**: Sustained attention is the practice that makes unconcealment possible. You can't reveal what's hidden without attending carefully.
- **Mneme ↔ Melete**: Memory holds what was experienced. Disciplined practice refines it into wisdom.
- **Organon → Nous**: The instruments serve the mind. Tools extend the agent's reach.
- **Agora → Nous**: Voices from the gathering place reach the mind. Channels feed the agent.

---

## Rejected names

| Name | Meaning | Why Rejected |
|------|---------|-------------|
| **Techne** (τέχνη) | Craft knowledge | Too generic. Every crate involves techne. |
| **Logos** (λόγος) | Rational principle | Too overloaded. Means everything from "word" to "cosmic reason." |
| **Sophia** (σοφία) | Wisdom | Aspirational rather than descriptive. The system pursues wisdom; it doesn't contain it. |
