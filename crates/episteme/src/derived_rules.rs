//! Derived-rule definitions for the knowledge engine.
//!
//! This module contains pure Datalog rule strings for three categories of
//! derived knowledge:
//!
//! - **Ontological rules** — type subsumption and IS-A transitivity over the
//!   `type_hierarchy` relation. Derived facts inherit the knowledge of their
//!   ancestor types.
//!
//! - **Causal-chain rules** — transitive causal closure with confidence decay.
//!   Replaces application-level BFS with a single recursive Datalog query.
//!
//! - **Defeasible defaults** — rules that hold for an entity unless a more
//!   specific exception overrides them. Specific facts (higher tier/confidence)
//!   defeat general defaults for the same entity.
//!
//! # Usage
//!
//! Call [`KnowledgeStore::materialize_derived_facts`] to run all rule sets and
//! write the resulting `derived_facts` tuples into the store. Each fact carries
//! the `rule_id` that produced it for traceability.
//!
//! The rule strings are exported as `pub(crate)` constants so that the
//! [`knowledge_store`](crate::knowledge_store) submodules can compose them
//! with relation-specific parameters without duplicating string literals.

// ── Ontological rules ─────────────────────────────────────────────────────────

/// Datalog rule: direct IS-A membership.
///
/// `is_a[child, parent]` holds when `type_hierarchy` has a direct edge.
/// This base case anchors the recursive transitivity rule below.
/// Composed into [`ONTOLOGICAL_MATERIALIZATION`] for production use.
#[expect(
    dead_code,
    reason = "building-block rule: composed into ONTOLOGICAL_MATERIALIZATION, exposed for documentation and future composable rule construction"
)]
pub(crate) const IS_A_BASE: &str = r"
is_a[child, parent] :=
    *type_hierarchy{child_type: child, parent_type: parent}
";

/// Datalog rule: transitive IS-A closure.
///
/// `is_a[child, ancestor]` holds when `child` IS-A `mid` AND `mid` IS-A
/// `ancestor` (recursively). `CozoDB` evaluates this to fixpoint.
///
/// WHY: Datalog recursion replaces hand-written BFS. The engine guarantees
/// termination when the `type_hierarchy` relation is acyclic (enforced by
/// insertion validation).
/// Composed into [`ONTOLOGICAL_MATERIALIZATION`] for production use.
#[expect(
    dead_code,
    reason = "building-block rule: composed into ONTOLOGICAL_MATERIALIZATION, exposed for documentation and future composable rule construction"
)]
pub(crate) const IS_A_TRANSITIVE: &str = r"
is_a[child, ancestor] :=
    *type_hierarchy{child_type: child, parent_type: mid},
    is_a[mid, ancestor]
";

/// Datalog script: materialize the `is_a` closure into `derived_facts`.
///
/// Produces rows `(entity_id, rule_id, derived_content, confidence)` where:
/// - `entity_id` is the entity whose type we resolved
/// - `rule_id` is `"ontological:is_a"` for traceability
/// - `derived_content` is `"type:<ancestor_type>"` (the inferred ancestor type)
/// - `confidence` is 1.0 (type hierarchy edges are definitional, not probabilistic)
///
/// The output feeds into the `:put derived_facts` write in
/// [`KnowledgeStore::materialize_ontological_rules`].
pub(crate) const ONTOLOGICAL_MATERIALIZATION: &str = r"
is_a[child, parent] :=
    *type_hierarchy{child_type: child, parent_type: parent}
is_a[child, ancestor] :=
    *type_hierarchy{child_type: child, parent_type: mid},
    is_a[mid, ancestor]

?[entity_id, rule_id, derived_content, confidence] :=
    *entities{id: entity_id, entity_type: etype},
    is_a[etype, ancestor],
    rule_id = 'ontological:is_a',
    derived_content = concat('type:', ancestor),
    confidence = 1.0
";

// ── Causal-chain rules ────────────────────────────────────────────────────────

/// Datalog script: materialize transitive causal closure into `derived_facts`.
///
/// Produces `(entity_id, rule_id, derived_content, confidence)` rows for all
/// indirect causal connections reachable from direct `causal_edges`.
///
/// Confidence decays multiplicatively along each hop: `conf = c1 * c2`.
/// A minimum threshold of 0.05 prunes negligible chains to bound output size.
///
/// WHY: replaces the application-level BFS in `propagate_confidence`. The
/// Datalog engine evaluates recursive rules to fixpoint in one query; the
/// application no longer needs to iteratively fetch edges.
pub(crate) const CAUSAL_CHAIN_MATERIALIZATION: &str = r"
causal_chain[cause, effect, conf] :=
    *causal_edges{cause: cause, effect: effect, confidence: conf}
causal_chain[cause, effect, conf] :=
    *causal_edges{cause: cause, effect: mid, confidence: c1},
    causal_chain[mid, effect, c2],
    conf = c1 * c2,
    conf >= 0.05

?[entity_id, rule_id, derived_content, confidence] :=
    causal_chain[cause, effect, conf],
    entity_id = cause,
    rule_id = 'causal:transitive_chain',
    derived_content = concat('causes:', effect),
    confidence = conf
";

// ── Defeasible-default rules ──────────────────────────────────────────────────

/// Datalog script: materialize defeasible defaults into `derived_facts`.
///
/// A default applies to an entity when no more-specific (higher-tier) fact
/// overrides it for the same entity. Tier ordering: `verified > assumed >
/// inferred`. The rule produces one derived row per active default.
///
/// The logic uses `CozoDB`'s negation-as-failure (`not`):
/// - Base rule `active_default[entity, content, conf]` fires for every
///   `defaults` row whose `entity_id` matches an entity.
/// - The override check suppresses it when a `facts` row for the same entity
///   has tier `"verified"` and content `str_includes` the default's tag.
///
/// WHY negation-as-failure over explicit priority numbers: the tier enum
/// already encodes the precedence hierarchy. Encoding it again as integers
/// would duplicate the schema invariant.
///
/// For production use the entity-scoped variant
/// [`DEFEASIBLE_SCOPED_MATERIALIZATION`] is preferred; this simpler variant
/// is retained for documentation and unit tests.
#[expect(
    dead_code,
    reason = "simplified variant retained for documentation; DEFEASIBLE_SCOPED_MATERIALIZATION is used in production"
)]
pub(crate) const DEFEASIBLE_MATERIALIZATION: &str = r"
active_default[entity_id, content, conf] :=
    *defaults{entity_id, default_content: content, confidence: conf},
    not *facts{content: override_content, nous_id: _},
    not *facts{content: override_content, tier: 'verified'}

?[entity_id, rule_id, derived_content, confidence] :=
    active_default[entity_id, content, conf],
    entity_id != '',
    rule_id = 'defeasible:default',
    derived_content = content,
    confidence = conf
";

/// Datalog script: materialize defeasible defaults with entity-scoped override check.
///
/// This is the production version used by
/// [`KnowledgeStore::materialize_defeasible_rules`]. It checks whether a
/// `verified` fact for the *same entity* already covers the default's tag
/// before emitting the derived row.
///
/// The inner negation scopes to `(entity_id, tag)` pairs: a verified fact for
/// entity `alice` does **not** suppress a default for entity `bob`.
///
/// `CozoDB` `not` is stratified negation: the `defaults` and `facts` base
/// relations must be fully evaluated before the outer rule fires. This holds
/// here because neither relation is derived by this script.
pub(crate) const DEFEASIBLE_SCOPED_MATERIALIZATION: &str = r"
verified_for_entity[entity_id, tag] :=
    *fact_entities{fact_id: fid, entity_id: entity_id},
    *facts{id: fid, valid_from: _vf, content: c, tier: 'verified'},
    *defaults{entity_id: entity_id, tag: tag},
    str_includes(c, tag)

?[entity_id, rule_id, derived_content, confidence] :=
    *defaults{entity_id: entity_id, tag: tag, default_content: content, confidence: conf},
    not verified_for_entity[entity_id, tag],
    rule_id = 'defeasible:default',
    derived_content = content,
    confidence = conf
";

// ── Combined materialization ──────────────────────────────────────────────────

/// All rule IDs emitted by the derived-rule engine.
///
/// Used to filter and inspect `derived_facts` rows by provenance.
pub const RULE_IDS: &[&str] = &[
    "ontological:is_a",
    "causal:transitive_chain",
    "defeasible:default",
];
