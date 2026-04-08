//! Intent persistence with conviction tiers for sustained autonomous governance.
//!
//! Stores operator intents as tiered facts so the nous can act on standing orders
//! across sessions. Higher-tier intents govern autonomous decisions; lower-tier ones
//! inform preferences without mandating behaviour.
//!
//! Intents are stored at `instance/nous/<agent>/intents.json` and loaded during
//! bootstrap for injection into the system prompt.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use aletheia_koina::ulid::Ulid;

use crate::error::{self, Result};

/// The strength with which an intent governs autonomous behaviour.
///
/// The tier controls:
/// - **Authorship**: only operators may add `Directive`; the nous may only add `Suggestion`.
/// - **Decay**: all tiers are decay-resistant (not subject to FSRS scheduling).
/// - **Override precedence**: `Directive` > `Preference` > `Suggestion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ConvictionTier {
    /// Must do: governs every autonomous decision in scope.
    ///
    /// Only the operator may add Directives. The nous treats them as inviolable
    /// constraints, not suggestions to weigh.
    Directive = 2,
    /// Should do: strong preference the nous applies unless blocked.
    ///
    /// Only the operator may add Preferences. The nous respects them unless a
    /// Directive or hard constraint requires otherwise.
    Preference = 1,
    /// May do: weak guidance the nous applies when convenient.
    ///
    /// Both operator and nous may add Suggestions. They act as lightweight
    /// nudges that can be overridden by any harder constraint or better option.
    Suggestion = 0,
}

/// Who authored this intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum IntentSource {
    /// Added by the human operator via explicit instruction.
    Operator,
    /// Added by the nous itself during autonomous operation.
    Nous,
}

/// A persisted operator intent that governs autonomous decisions.
///
/// Intents are time-bounded standing orders. The nous consults active intents
/// before every autonomous decision: planning, dispatch ordering, merge priority,
/// and attention allocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    /// Unique intent identifier.
    pub id: Ulid,
    /// Human-readable description of what the intent governs.
    ///
    /// Example: "Prioritise kanon fixes over new features until Phase 04b lands."
    pub description: String,
    /// How strongly this intent governs autonomous behaviour.
    pub conviction_tier: ConvictionTier,
    /// Who created this intent.
    pub source: IntentSource,
    /// When the intent was created.
    pub created_at: jiff::Timestamp,
    /// When the intent expires, if ever.
    ///
    /// `None` means the intent is open-ended and must be explicitly resolved.
    pub expires_at: Option<jiff::Timestamp>,
    /// Whether the intent has been explicitly resolved.
    ///
    /// Resolved intents are retained for audit purposes but excluded from the
    /// active set consulted during bootstrap.
    pub resolved: bool,
}

impl Intent {
    /// Create a new intent.
    ///
    /// # Panics
    ///
    /// Panics if `source == IntentSource::Nous` and `conviction_tier` is not
    /// [`ConvictionTier::Suggestion`]. The nous may only record suggestions —
    /// directives and preferences are operator-only.
    #[must_use]
    pub fn new(
        description: String,
        conviction_tier: ConvictionTier,
        source: IntentSource,
        expires_at: Option<jiff::Timestamp>,
    ) -> Self {
        assert!(
            !(source == IntentSource::Nous && conviction_tier != ConvictionTier::Suggestion),
            "nous may only add Suggestion-tier intents; \
             Directive and Preference are operator-only"
        );
        Self {
            id: Ulid::new(),
            description,
            conviction_tier,
            source,
            created_at: jiff::Timestamp::now(),
            expires_at,
            resolved: false,
        }
    }

    /// Return `true` if the intent is active: not resolved and not expired.
    #[must_use]
    pub fn is_active(&self) -> bool {
        if self.resolved {
            return false;
        }
        match self.expires_at {
            Some(expiry) => jiff::Timestamp::now() < expiry,
            None => true,
        }
    }
}

/// Filename used for the on-disk intent store.
const INTENTS_JSON: &str = "intents.json";

/// Persistent store for operator intents.
///
/// Backed by `instance/nous/<agent>/intents.json`. All mutating operations
/// write through to disk immediately — there is no in-memory cache to go stale.
///
/// Intents are not subject to FSRS decay. They persist until explicitly resolved
/// or their `expires_at` timestamp passes.
pub struct IntentStore {
    path: PathBuf,
}

impl IntentStore {
    /// Open (or create) an intent store for the given agent workspace directory.
    ///
    /// The store file is created with 0600 permissions on first write.
    /// If the file does not yet exist, `list_intents` returns an empty vec.
    #[must_use]
    pub fn new(agent_dir: impl Into<PathBuf>) -> Self {
        Self {
            path: agent_dir.into().join(INTENTS_JSON),
        }
    }

    /// Build the canonical store path for an agent.
    ///
    /// Given an `instance_root` (e.g. `instance/`) and an `agent_id`, returns
    /// `instance/nous/<agent_id>/intents.json`.
    #[must_use]
    pub fn path_for(instance_root: &Path, agent_id: &str) -> PathBuf {
        instance_root.join("nous").join(agent_id).join(INTENTS_JSON)
    }

    /// Add a new intent and persist it immediately.
    ///
    /// # Errors
    ///
    /// Returns a workspace I/O or serialization error if the store cannot be written.
    pub fn add_intent(&self, intent: Intent) -> Result<Intent> {
        let mut intents = self.load_all()?;
        intents.push(intent.clone());
        self.persist(&intents)?;
        Ok(intent)
    }

    /// Return all intents (active and resolved).
    ///
    /// # Errors
    ///
    /// Returns a deserialization error if the store file is malformed.
    pub fn list_intents(&self) -> Result<Vec<Intent>> {
        self.load_all()
    }

    /// Return only active (non-resolved, non-expired) intents, sorted by conviction tier descending.
    ///
    /// Directives appear first, then Preferences, then Suggestions.
    ///
    /// # Errors
    ///
    /// Returns a deserialization error if the store file is malformed.
    pub fn active_intents(&self) -> Result<Vec<Intent>> {
        let mut active: Vec<Intent> = self
            .load_all()?
            .into_iter()
            .filter(Intent::is_active)
            .collect();
        // WHY: Directives must appear before Preferences before Suggestions so that
        // consumers see the highest-conviction intent first without additional sorting.
        active.sort_by(|a, b| b.conviction_tier.cmp(&a.conviction_tier));
        Ok(active)
    }

    /// Expire all intents whose `expires_at` has passed.
    ///
    /// This does not mark them as resolved — expired intents are simply inactive.
    /// Returns the number of intents whose state changed from active to expired.
    ///
    /// # Errors
    ///
    /// Returns a workspace I/O error if the updated store cannot be written.
    pub fn expire_intents(&self) -> Result<usize> {
        let intents = self.load_all()?;
        let now = jiff::Timestamp::now();
        let expired_count = intents
            .iter()
            .filter(|i| {
                !i.resolved
                    && i.expires_at
                        .is_some_and(|exp| exp <= now)
            })
            .count();
        // WHY: no structural change needed — is_active() checks expires_at at read time.
        // We write back to canonicalise the file (e.g. after an external edit).
        self.persist(&intents)?;
        Ok(expired_count)
    }

    /// Mark an intent as resolved by its ID.
    ///
    /// Resolved intents remain in the store for audit purposes but are excluded
    /// from the active set used during bootstrap.
    ///
    /// Returns `true` if the intent was found and updated, `false` if not found.
    ///
    /// # Errors
    ///
    /// Returns a workspace I/O error if the updated store cannot be written.
    pub fn resolve_intent(&self, id: Ulid) -> Result<bool> {
        let mut intents = self.load_all()?;
        let Some(intent) = intents.iter_mut().find(|i| i.id == id) else {
            return Ok(false);
        };
        intent.resolved = true;
        self.persist(&intents)?;
        Ok(true)
    }

    /// Render active intents as a markdown section for system prompt injection.
    ///
    /// Returns `None` if there are no active intents (no section to inject).
    ///
    /// # Errors
    ///
    /// Returns a deserialization error if the store file is malformed.
    pub fn render_for_bootstrap(&self) -> Result<Option<String>> {
        let active = self.active_intents()?;
        if active.is_empty() {
            return Ok(None);
        }

        let mut out = String::from(
            "Standing orders from the operator. \
             Consult these before every autonomous decision.\n\n",
        );

        for intent in &active {
            let tier_label = match intent.conviction_tier {
                ConvictionTier::Directive => "[DIRECTIVE — must do]",
                ConvictionTier::Preference => "[PREFERENCE — should do]",
                ConvictionTier::Suggestion => "[SUGGESTION — may do]",
            };
            let source_label = match intent.source {
                IntentSource::Operator => "operator",
                IntentSource::Nous => "nous",
            };
            out.push_str(&format!(
                "- {} {} (set by {}, {})\n",
                tier_label,
                intent.description,
                source_label,
                intent.created_at,
            ));
        }

        Ok(Some(out))
    }

    /// Load all intents from disk.
    fn load_all(&self) -> Result<Vec<Intent>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let contents =
            std::fs::read_to_string(&self.path).context(error::WorkspaceIoSnafu {
                path: self.path.clone(),
            })?;
        let intents: Vec<Intent> =
            serde_json::from_str(&contents).context(error::WorkspaceDeserializeSnafu)?;
        Ok(intents)
    }

    /// Persist the full intent list to disk atomically with 0600 permissions.
    fn persist(&self, intents: &[Intent]) -> Result<()> {
        let json =
            serde_json::to_string_pretty(intents).context(error::WorkspaceSerializeSnafu)?;
        aletheia_koina::fs::write_restricted(&self.path, json.as_bytes()).context(
            error::WorkspaceIoSnafu {
                path: self.path.clone(),
            },
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn make_store() -> (tempfile::TempDir, IntentStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = IntentStore::new(dir.path());
        (dir, store)
    }

    fn operator_directive(desc: &str) -> Intent {
        Intent::new(
            desc.into(),
            ConvictionTier::Directive,
            IntentSource::Operator,
            None,
        )
    }

    fn operator_preference(desc: &str) -> Intent {
        Intent::new(
            desc.into(),
            ConvictionTier::Preference,
            IntentSource::Operator,
            None,
        )
    }

    fn nous_suggestion(desc: &str) -> Intent {
        Intent::new(
            desc.into(),
            ConvictionTier::Suggestion,
            IntentSource::Nous,
            None,
        )
    }

    // --- IntentStore CRUD ---

    #[test]
    fn add_and_list_roundtrip() {
        let (_dir, store) = make_store();
        let intent = operator_directive("prioritise kanon over new features");
        store.add_intent(intent.clone()).unwrap();

        let intents = store.list_intents().unwrap();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].description, intent.description);
        assert_eq!(intents[0].conviction_tier, ConvictionTier::Directive);
        assert_eq!(intents[0].source, IntentSource::Operator);
        assert!(!intents[0].resolved);
    }

    #[test]
    fn multiple_intents_accumulated() {
        let (_dir, store) = make_store();
        store.add_intent(operator_directive("directive one")).unwrap();
        store
            .add_intent(operator_preference("preference one"))
            .unwrap();
        store.add_intent(nous_suggestion("suggestion one")).unwrap();

        let intents = store.list_intents().unwrap();
        assert_eq!(intents.len(), 3);
    }

    #[test]
    fn resolve_intent_marks_resolved() {
        let (_dir, store) = make_store();
        let intent = store
            .add_intent(operator_directive("ship thumos phase 03"))
            .unwrap();

        let found = store.resolve_intent(intent.id).unwrap();
        assert!(found);

        let intents = store.list_intents().unwrap();
        assert!(intents[0].resolved);
    }

    #[test]
    fn resolve_unknown_id_returns_false() {
        let (_dir, store) = make_store();
        let unknown_id = Ulid::new();
        let found = store.resolve_intent(unknown_id).unwrap();
        assert!(!found);
    }

    #[test]
    fn list_empty_store_returns_empty_vec() {
        let (_dir, store) = make_store();
        let intents = store.list_intents().unwrap();
        assert!(intents.is_empty());
    }

    // --- Active / expiry ---

    #[test]
    fn resolved_intent_not_active() {
        let (_dir, store) = make_store();
        let intent = store
            .add_intent(operator_directive("do not ship features"))
            .unwrap();
        store.resolve_intent(intent.id).unwrap();

        let active = store.active_intents().unwrap();
        assert!(active.is_empty());
    }

    #[test]
    fn expired_intent_not_active() {
        let (_dir, store) = make_store();
        // expires_at in the past
        let past = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_secs(1))
            .unwrap();
        let mut intent = Intent::new(
            "no longer relevant".into(),
            ConvictionTier::Suggestion,
            IntentSource::Operator,
            Some(past),
        );
        // Force past expiry — new() uses now() so we patch after construction
        intent.expires_at = Some(past);
        store.add_intent(intent).unwrap();

        let active = store.active_intents().unwrap();
        assert!(active.is_empty());
    }

    #[test]
    fn future_expiry_intent_is_active() {
        let (_dir, store) = make_store();
        let future = jiff::Timestamp::now()
            .checked_add(jiff::SignedDuration::from_secs(3600))
            .unwrap();
        let intent = Intent::new(
            "active for the next hour".into(),
            ConvictionTier::Preference,
            IntentSource::Operator,
            Some(future),
        );
        store.add_intent(intent).unwrap();

        let active = store.active_intents().unwrap();
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn expire_intents_returns_count() {
        let (_dir, store) = make_store();

        let past = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_secs(1))
            .unwrap();
        let mut expired = Intent::new(
            "expired".into(),
            ConvictionTier::Suggestion,
            IntentSource::Operator,
            Some(past),
        );
        expired.expires_at = Some(past);
        store.add_intent(expired).unwrap();
        store
            .add_intent(operator_directive("still active"))
            .unwrap();

        let count = store.expire_intents().unwrap();
        assert_eq!(count, 1);
    }

    // --- Ordering ---

    #[test]
    fn active_intents_sorted_directive_first() {
        let (_dir, store) = make_store();
        store.add_intent(nous_suggestion("low priority hint")).unwrap();
        store
            .add_intent(operator_preference("medium priority"))
            .unwrap();
        store
            .add_intent(operator_directive("highest priority"))
            .unwrap();

        let active = store.active_intents().unwrap();
        assert_eq!(active.len(), 3);
        assert_eq!(active[0].conviction_tier, ConvictionTier::Directive);
        assert_eq!(active[1].conviction_tier, ConvictionTier::Preference);
        assert_eq!(active[2].conviction_tier, ConvictionTier::Suggestion);
    }

    // --- Bootstrap rendering ---

    #[test]
    fn render_for_bootstrap_empty_returns_none() {
        let (_dir, store) = make_store();
        let result = store.render_for_bootstrap().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn render_for_bootstrap_includes_active_intents() {
        let (_dir, store) = make_store();
        store
            .add_intent(operator_directive("ship kanon Phase 04b this month"))
            .unwrap();
        store
            .add_intent(nous_suggestion("prefer small PRs"))
            .unwrap();

        let md = store.render_for_bootstrap().unwrap().unwrap();
        assert!(md.contains("[DIRECTIVE — must do]"));
        assert!(md.contains("ship kanon Phase 04b this month"));
        assert!(md.contains("[SUGGESTION — may do]"));
        assert!(md.contains("prefer small PRs"));
    }

    #[test]
    fn render_for_bootstrap_excludes_resolved_intents() {
        let (_dir, store) = make_store();
        let intent = store
            .add_intent(operator_directive("do not merge feat branches"))
            .unwrap();
        store.resolve_intent(intent.id).unwrap();

        let result = store.render_for_bootstrap().unwrap();
        assert!(result.is_none(), "resolved intent should not appear in bootstrap");
    }

    // --- Source / tier access control ---

    #[test]
    fn nous_can_add_suggestion() {
        let (_dir, store) = make_store();
        let intent = nous_suggestion("batch small fixes before standups");
        let added = store.add_intent(intent).unwrap();
        assert_eq!(added.source, IntentSource::Nous);
        assert_eq!(added.conviction_tier, ConvictionTier::Suggestion);
    }

    #[test]
    #[should_panic(expected = "nous may only add Suggestion-tier intents")]
    fn nous_cannot_add_directive() {
        let _ = Intent::new(
            "override operator".into(),
            ConvictionTier::Directive,
            IntentSource::Nous,
            None,
        );
    }

    #[test]
    #[should_panic(expected = "nous may only add Suggestion-tier intents")]
    fn nous_cannot_add_preference() {
        let _ = Intent::new(
            "prefer X over Y".into(),
            ConvictionTier::Preference,
            IntentSource::Nous,
            None,
        );
    }

    // --- Persistence ---

    #[test]
    fn intents_survive_store_reopen() {
        let dir = tempfile::tempdir().unwrap();
        {
            let store = IntentStore::new(dir.path());
            store
                .add_intent(operator_directive("persist across restarts"))
                .unwrap();
        }
        // Re-open a new IntentStore pointing at the same directory
        let store2 = IntentStore::new(dir.path());
        let intents = store2.list_intents().unwrap();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].description, "persist across restarts");
    }

    #[test]
    fn store_file_has_restricted_permissions() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let dir = tempfile::tempdir().unwrap();
            let store = IntentStore::new(dir.path());
            store
                .add_intent(operator_directive("permissions test"))
                .unwrap();
            let meta = std::fs::metadata(dir.path().join("intents.json")).unwrap();
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "intents.json should be 0600");
        }
    }

    #[test]
    fn path_for_constructs_expected_path() {
        let root = Path::new("/instance");
        let path = IntentStore::path_for(root, "syn");
        assert_eq!(path, PathBuf::from("/instance/nous/syn/intents.json"));
    }

    // --- ConvictionTier ordering ---

    #[test]
    fn conviction_tier_ordering() {
        assert!(ConvictionTier::Directive > ConvictionTier::Preference);
        assert!(ConvictionTier::Preference > ConvictionTier::Suggestion);
    }
}
