# Glossary

Project-specific terms used across the Aletheia codebase. For the naming philosophy and
Greek construction system, see [gnomon.md](gnomon.md). For the full crate registry with
layer-by-layer analysis, see [lexicon.md](lexicon.md).

Each entry gives: Greek word and pronunciation, etymology, technical meaning in this
codebase, and cross-reference to the relevant crate or module.

---

## Aletheia

**ἀλήθεια** (*a-lé-thei-a*)

**Etymology.** Compound of *ἀ-* (negation) + *λήθη* (concealment, forgetting). Literally
"not-hidden" or "unconcealment." Heidegger reads it as truth understood not as correspondence
to facts but as the *revealing of what was hidden* — the act of bringing forth from
concealment.

**In this codebase.** The project name and the binary crate. The system as a whole: agents,
memory, orchestration, multi-nous cognition. The name asserts that the system's essential
act is unconcealment — surfacing latent knowledge, making hidden patterns legible, refusing
to let things stay forgotten.

**Crate.** `aletheia` (binary, top layer) — CLI, `serve` mode, startup.

---

## Agora

**ἀγορά** (*a-go-rá*)

**Etymology.** The Greek civic gathering place: the open space where citizens assembled to
speak, trade, and deliberate. From *ἀγείρω* (to gather together).

**In this codebase.** The channel registry and messaging layer. Registers `ChannelProvider`
implementations (e.g. Signal JSON-RPC client) and routes messages between external
communication channels and the agent pipeline.

**Crate.** `agora` (mid layer).

---

## Dianoia

**διάνοια** (*di-á-noi-a*)

**Etymology.** From *διά* (through, across) + *νοῦς* (mind). Plato's term for discursive
reasoning: the mode of thought that works *through* problems step by step, as opposed to
noesis (immediate insight). In the divided line of *Republic* VI, dianoia occupies the
upper-sensible region — reasoning that still relies on hypotheses and images rather than
grasping Forms directly.

**In this codebase.** The multi-phase planning orchestrator. Implements structured reasoning
that breaks complex tasks into steps, tracks project context, and coordinates sequential
execution.

**Crate.** `dianoia` (mid layer) — planning orchestrator.

---

## Diaporeia

**διαπορεία** (*di-a-po-rei-a*)

**Etymology.** From *διά* (through, across) + *πορεία* (journey, passage). Transit between
worlds; the act of passing through.

**In this codebase.** The Model Context Protocol (MCP) server bridge. Exposes Aletheia tools
and knowledge to external AI agents via the MCP protocol, bridging the internal system to the
outside.

**Module.** Internal module within the plugin layer.

---

## Dokimion

**δοκίμιον** (*do-kí-mi-on*)

**Etymology.** From *δοκιμάζω* (to test, to put to proof). The test or proof that
demonstrates whether something is genuine — the assay that distinguishes gold from dross.

**In this codebase.** The behavioral evaluation framework. Runs HTTP scenario tests against a
live Aletheia instance to verify agent behavior matches specification.

**Crate.** `dokimion` (mid layer) — eval runner.

---

## Eidos

**εἶδος** (*eí-dos*)

**Etymology.** From *εἴδω* (to see, to know). The visible form: the essential shape that
makes a thing *this kind* of thing rather than another. Plato's term for the intelligible
forms; Aristotle's term for the formal cause.

**In this codebase.** The shared memory type library. Defines `Fact`, `Session`, `NousId`,
and other domain types — the forms that make a fact a Fact, a session a Session.

**Crate.** `eidos` (low layer) — shared memory types.

---

## Episteme

**ἐπιστήμη** (*e-pi-stí-mi*)

**Etymology.** From *ἐπί* (upon, toward) + *ἵστημι* (to stand). Systematic, grounded
knowledge: knowledge that *stands upon* demonstrated foundations, as opposed to mere opinion
or acquaintance. Aristotle distinguishes it from techne (craft knowledge) and phronesis
(practical wisdom).

**In this codebase.** The knowledge extraction and recall system. Transforms raw conversation
into structured facts: extraction, deduplication, confidence scoring, succession tracking.

**Crate.** `episteme` (low layer) — knowledge engine.

---

## Graphe

**γραφή** (*gra-fí*)

**Etymology.** From *γράφω* (to write, to inscribe). The inscription; writing as the
preservation of what was said.

**In this codebase.** The session persistence layer. Stores conversation messages and turn
history in SQLite — the written record of what passed between user and agent.

**Crate.** `graphe` (low layer) — session persistence.

---

## Hermeneus

**ἑρμηνεύς** (*her-me-neús*)

**Etymology.** The interpreter or translator; one who carries meaning across worlds. From
Hermes, messenger of the gods, who moved between divine and mortal realms. The art of
hermeneutics (interpretation) derives from the same root.

**In this codebase.** The LLM provider abstraction layer. Wraps the Anthropic Messages API
(and other backends), handles model routing, credential management, and streaming. The
translation layer between nous (agent logic) and the underlying language model.

**Crate.** `hermeneus` (low layer) — provider client.

---

## Koina

**κοινά** (*koi-ná*)

**Etymology.** Plural of *κοινός* (common, shared, public). The commons: what belongs to
all, what is held in common. Greek cities maintained *ta koina* — the common things, the
public goods.

**In this codebase.** The shared utility library. Error types (snafu integration), tracing
helpers, filesystem utilities, HTTP constants, safe wrappers. Everything the rest of the
workspace uses without it belonging to any single layer.

**Crate.** `koina` (leaf layer) — shared utilities.

---

## Krites

**κριτής** (*kri-tís*)

**Etymology.** From *κρίνω* (to separate, to judge, to decide). The judge: one who
distinguishes, evaluates, and renders a verdict.

**In this codebase.** The Datalog query engine. A vendored CozoDB instance that evaluates
logical rules and facts, deciding what follows from what the system knows.

**Crate.** `krites` (low layer) — Datalog engine.

---

## Melete

**μελέτη** (*me-lé-ti*)

**Etymology.** From *μελετάω* (to care for, to practice). Disciplined care and practice;
one of the three original Muses (alongside Mneme and Aoide). The Stoics used it for
preparatory meditation — rehearsing what matters before it is needed.

**In this codebase.** The context distillation layer. Compresses conversation history to fit
within token budgets, preserving what matters while releasing what can be released.

**Crate.** `melete` (low layer) — distillation and compression.

---

## Mneme

**μνήμη** (*mní-mi*)

**Etymology.** Memory as active faculty; the Muse of memory. From *μνάομαι* (to remember,
to be mindful of). One of the original three Muses. Plato's *anamnesis* (recollection) uses
the same root: learning as remembering what the soul already knows.

**In this codebase.** The memory engine facade. Re-exports types and operations from `eidos`,
`krites`, `graphe`, and `episteme` into a single coherent API. Not passive storage but an
active faculty — memory that recalls, scores, and surfaces what is relevant.

**Crate.** `mneme` (low layer) — memory facade.

---

## Nous

**νοῦς** (*noús*)

**Etymology.** Direct apprehension or intellection; the highest mode of knowing in Greek
philosophy. Distinct from dianoia (discursive reasoning) and episteme (systematic knowledge).
Aristotle's *nous* grasps first principles immediately, without inference. Plotinus placed it
as the second hypostasis — the Divine Mind.

**In this codebase.** The agent pipeline and actor. Each nous is an autonomous agent with its
own identity, memory, and tool set. The pipeline: bootstrap context → recall memories →
execute turn (LLM + tools) → finalize and store. Also the Tokio actor (`NousActor`) that
serializes concurrent requests.

**Crate.** `nous` (mid layer) — agent identity and pipeline.

---

## Oikos

**οἶκος** (*oí-kos*)

**Etymology.** Household; the fundamental unit of Greek economic and social life. From *oikos*
comes *oikonomia* (household management) and hence *economy*.

**In this codebase.** The instance directory layout — the "household" of a running Aletheia
installation. `Oikos` is a struct in the `taxis` crate that resolves canonical paths for
data, config, logs, backups, and workspace directories within the instance root. The theke
(vault) lives inside the oikos.

**Module.** Defined in `taxis` (config/path resolution crate); referenced throughout as
`AppState.oikos`.

---

## Oikonomos

**οἰκονόμος** (*oi-ko-nó-mos*)

**Etymology.** From *οἶκος* (household) + *νέμω* (to manage, to distribute). The household
steward: the one who keeps the oikos in order, allocates resources, and ensures continuity.

**In this codebase.** The background lifecycle and maintenance layer. Scheduling, cron-like
task runners, startup/shutdown sequencing, and periodic maintenance jobs (trace rotation,
drift detection, database monitoring).

**Crate path.** `crates/daemon/` — maintenance task runner; maps to the `oikonomos` concept
in the lexicon.

---

## Organon

**ὄργανον** (*ór-ga-non*)

**Etymology.** Instrument or tool; that by which something is done. Aristotle named his
collected logical treatises the *Organon* — the instrument of reasoning, the tool of thought.
The name captures that logic is not knowledge itself but the instrument for acquiring it.

**In this codebase.** The tool registry and executor. Defines the `ToolExecutor` trait,
registers built-in tools (file operations, shell commands, HTTP requests, etc.), and
dispatches tool calls from the agent. Tools are the instruments by which nous extends its
reach into the world.

**Crate.** `organon` (low layer) — tool registry and built-ins.

---

## Prosoche

**προσοχή** (*pro-so-hí*)

**Etymology.** From *πρός* (toward) + *ἔχω* (to hold). Holding-toward; sustained directed
attention. The Stoics, particularly Epictetus and Marcus Aurelius, used it for the
disciplined practice of attending carefully to one's inner state and present circumstances.

**In this codebase.** The health monitoring and attention layer. Checks system state, monitors
agent health, surfaces directive-level concerns. The practice that makes unconcealment
(aletheia) possible — you cannot reveal what is hidden without attending carefully.

**Module.** Internal module within the daemon layer.

---

## Pylon

**πυλών** (*py-lón*)

**Etymology.** The gate-tower or monumental gateway; the architectural structure marking the
boundary between inside and outside. In ancient Egypt and Greece, the pylon was the imposing
entrance to a temple precinct — the threshold between the profane and sacred worlds.

**In this codebase.** The HTTP API gateway. Axum-based server with SSE streaming, JWT
authentication, CSRF protection, rate limiting, and the full middleware stack. The
architectural boundary between the outside world (clients, TUI, external agents) and the
internal Aletheia runtime.

**Crate.** `pylon` (high layer) — HTTP gateway. See also [crates/pylon/docs/handlers.md](../crates/pylon/docs/handlers.md).

---

## Symbolon

**σύμβολον** (*sým-bo-lon*)

**Etymology.** From *συμβάλλω* (to throw together, to bring into contact). The token of
recognition: in antiquity, two parties would break a piece of pottery and each keep half; the
reunion of the halves (*symbolon*) proved identity and established trust. The origin of
"symbol" in English.

**In this codebase.** The authentication and credential layer. JWT token issuance and
validation, password hashing (argon2), and RBAC policies. Identity proven by presenting the
matching half of a shared secret.

**Crate.** `symbolon` (leaf layer) — auth and credentials.

---

## Taxis

**τάξις** (*tá-xis*)

**Etymology.** From *τάσσω* (to arrange, to put in order). The arrangement that makes a
collection coherent; ordered structure. Aristotle used *taxis* for the organization that
distinguishes a well-ordered army from a mob. In rhetoric, it refers to the arrangement of
arguments.

**In this codebase.** The configuration loading and path resolution crate. Implements a
figment cascade (defaults → TOML → environment variables), resolves the `Oikos` instance
directory layout, and exposes `AletheiaConfig` to the rest of the system.

**Crate.** `taxis` (low layer) — config and path resolution.

---

## Theke

**θήκη** (*thí-ki*)

**Etymology.** A repository, chest, or case — a place that holds what matters. From *τίθημι*
(to place, to put). Used for the chest that preserved valuable documents or objects.

**In this codebase.** The tier-0 instance vault: the human-and-nous collaborative workspace
within the instance directory. A structured space where operator-curated documents, agent
workspace files, and shared knowledge are stored. The theke lives inside the oikos.

**Module.** Internal module; the vault subdirectory within the instance `Oikos`.

---

## Thesauros

**θησαυρός** (*the-sau-rós*)

**Etymology.** Treasury or storehouse; a place where accumulated wealth is held ready for
use. The Greek word gives English "thesaurus" — a treasury of words.

**In this codebase.** The domain pack loader. Loads knowledge packs (curated bundles of
domain knowledge, tool configurations, and config overlays) and makes them available to
agents. A storehouse of accumulated domain expertise held ready for use.

**Crate.** `thesauros` (mid layer) — pack loader.

---

## Key topological relationships

```
Prosoche ──► Aletheia          Sustained attention enables unconcealment
Mneme ◄──► Melete              Memory holds experience; practice refines it
Nous ◄──► Dianoia              Immediate apprehension and discursive reasoning
Organon ──► Nous               Instruments serve the mind
Agora ──► Nous                 Channels feed the agent
Oikonomos manages Oikos        The steward keeps the household
Pylon guards the boundary      Outside world reaches Nous only through Pylon
```

---

## Further reading

- [gnomon.md](gnomon.md) — Naming philosophy, construction system, layer test (L1–L4)
- [lexicon.md](lexicon.md) — Full crate registry with L1–L4 analysis for each name
- [ARCHITECTURE.md](ARCHITECTURE.md) — Crate workspace, module map, dependency graph
