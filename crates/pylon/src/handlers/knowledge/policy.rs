//! Shared authorization policy for knowledge read endpoints.

use std::collections::BTreeSet;

use symbolon::types::Role;

use crate::error::ApiError;
use crate::extract::Claims;
use crate::state::KnowledgeState;

/// Resolved read policy for a single knowledge request.
#[derive(Debug, Clone)]
pub(super) struct KnowledgeReadPolicy<'a> {
    claims: &'a Claims,
    target_nous_ids: Option<BTreeSet<String>>,
}

impl<'a> KnowledgeReadPolicy<'a> {
    /// Build a policy for endpoints with an optional single `nous_id` filter.
    pub(super) fn from_single_nous(
        claims: &'a Claims,
        requested_nous_id: Option<&str>,
    ) -> Result<Self, ApiError> {
        let target_nous_ids = match (claims.nous_id.as_deref(), requested_nous_id) {
            (Some(scoped), Some(requested)) if scoped != requested => {
                return Err(ApiError::forbidden("access denied for this agent"));
            }
            (Some(scoped), _) => Some(BTreeSet::from([scoped.to_owned()])),
            (None, _) if claims.role == Role::Agent => {
                return Err(ApiError::forbidden(
                    "agent token must be scoped to a nous_id",
                ));
            }
            (None, Some(requested)) => Some(BTreeSet::from([requested.to_owned()])),
            (None, None) => None,
        };

        Ok(Self {
            claims,
            target_nous_ids,
        })
    }

    /// Build a policy for direct ID reads where the fact/entity carries policy fields.
    pub(super) fn from_claims(claims: &'a Claims) -> Result<Self, ApiError> {
        if claims.role == Role::Agent && claims.nous_id.is_none() {
            return Err(ApiError::forbidden(
                "agent token must be scoped to a nous_id",
            ));
        }

        Ok(Self {
            claims,
            target_nous_ids: None,
        })
    }

    /// Build a policy for endpoints with repeated agent filters.
    pub(super) fn from_agent_filters(
        claims: &'a Claims,
        requested_agents: &[String],
    ) -> Result<Self, ApiError> {
        let target_nous_ids = if let Some(scoped) = claims.nous_id.as_deref() {
            if requested_agents
                .iter()
                .any(|requested| requested.as_str() != scoped)
            {
                return Err(ApiError::forbidden("access denied for this agent"));
            }
            Some(BTreeSet::from([scoped.to_owned()]))
        } else if claims.role == Role::Agent {
            return Err(ApiError::forbidden(
                "agent token must be scoped to a nous_id",
            ));
        } else if requested_agents.is_empty() {
            None
        } else {
            Some(requested_agents.iter().cloned().collect())
        };

        Ok(Self {
            claims,
            target_nous_ids,
        })
    }

    /// Return the sole target nous id when this policy has exactly one.
    pub(super) fn single_target_nous_id(&self) -> Option<&str> {
        let targets = self.target_nous_ids.as_ref()?;
        if targets.len() == 1 {
            targets.iter().next().map(String::as_str)
        } else {
            None
        }
    }

    /// Return target filters as a sorted vector.
    pub(super) fn target_agents(&self) -> Vec<String> {
        self.target_nous_ids
            .as_ref()
            .map(|targets| targets.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Whether this request can read every fact without per-fact filtering.
    pub(super) fn allows_all_facts(&self) -> bool {
        self.is_unscoped_operator() && self.target_nous_ids.is_none()
    }

    /// Whether the caller has unscoped operator-level visibility.
    pub(super) fn is_unscoped_operator(&self) -> bool {
        self.claims.role >= Role::Operator && self.claims.nous_id.is_none()
    }

    /// Check fact visibility using hydrated fact fields.
    #[cfg(feature = "knowledge-store")]
    pub(super) fn allows_fact(&self, fact: &mneme::knowledge::Fact) -> bool {
        self.allows_fact_parts(&fact.nous_id, fact.visibility)
    }

    /// Check fact visibility from row-level owner and visibility fields.
    #[cfg(feature = "knowledge-store")]
    pub(super) fn allows_fact_parts(
        &self,
        fact_nous_id: &str,
        visibility: mneme::knowledge::Visibility,
    ) -> bool {
        if !self.matches_target(fact_nous_id) {
            return false;
        }

        if self.claims.role >= Role::Operator && self.claims.nous_id.is_none() {
            return true;
        }

        if self.claims.role == Role::Agent && self.claims.nous_id.is_none() {
            return false;
        }

        let own_fact = self
            .claims
            .nous_id
            .as_deref()
            .is_some_and(|scoped| scoped == fact_nous_id);
        let shared_fact = matches!(
            visibility,
            mneme::knowledge::Visibility::Shared | mneme::knowledge::Visibility::Published
        );

        own_fact || shared_fact
    }

    /// Filter a fact list to the records visible under this policy.
    #[cfg(feature = "knowledge-store")]
    pub(super) fn filter_facts(
        &self,
        facts: Vec<mneme::knowledge::Fact>,
    ) -> Vec<mneme::knowledge::Fact> {
        if self.allows_all_facts() {
            return facts;
        }
        facts
            .into_iter()
            .filter(|fact| self.allows_fact(fact))
            .collect()
    }

    /// Require visibility for a single fact.
    #[cfg(feature = "knowledge-store")]
    pub(super) fn require_fact(&self, fact: &mneme::knowledge::Fact) -> Result<(), ApiError> {
        if self.allows_fact(fact) {
            Ok(())
        } else {
            Err(ApiError::forbidden("access denied for this fact"))
        }
    }

    /// Require visibility for an entity through at least one visible linked fact.
    pub(super) fn require_entity(
        &self,
        state: &KnowledgeState,
        entity_id: &str,
    ) -> Result<(), ApiError> {
        if let Some(visible_ids) = visible_entity_ids(state, self)?
            && !visible_ids.contains(entity_id)
        {
            return Err(ApiError::NotFound {
                path: format!("entity/{entity_id}"),
                location: snafu::location!(),
            });
        }
        Ok(())
    }

    #[cfg(feature = "knowledge-store")]
    fn matches_target(&self, fact_nous_id: &str) -> bool {
        self.target_nous_ids
            .as_ref()
            .is_none_or(|targets| targets.contains(fact_nous_id))
    }
}

/// Return visible entity ids, or `None` when the caller may see every entity.
#[cfg_attr(
    not(feature = "knowledge-store"),
    expect(
        clippy::unnecessary_wraps,
        reason = "WHY: knowledge-store builds can fail while default builds keep the same API shape"
    )
)]
pub(super) fn visible_entity_ids(
    state: &KnowledgeState,
    policy: &KnowledgeReadPolicy<'_>,
) -> Result<Option<BTreeSet<String>>, ApiError> {
    if policy.allows_all_facts() {
        return Ok(None);
    }

    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let rows = store
            .run_query(
                r"
                ?[entity_id, nous_id, visibility] :=
                    *fact_entities{fact_id, entity_id},
                    *facts{id: fact_id, nous_id, visibility, is_forgotten, superseded_by},
                    is_forgotten == false,
                    is_null(superseded_by)
                ",
                std::collections::BTreeMap::new(),
            )
            .map_err(|e| ApiError::Internal {
                message: e.to_string(),
                location: snafu::location!(),
            })?;

        let mut visible = BTreeSet::new();
        for row in 0..rows.row_count() {
            let Some(entity_id) = rows.get_string(row, "entity_id") else {
                continue;
            };
            let Some(nous_id) = rows.get_string(row, "nous_id") else {
                continue;
            };
            let visibility = visibility_from_row(rows.get_string(row, "visibility").as_deref());
            if policy.allows_fact_parts(&nous_id, visibility) {
                visible.insert(entity_id);
            }
        }
        return Ok(Some(visible));
    }

    #[cfg(not(feature = "knowledge-store"))]
    let _ = state;

    Ok(Some(BTreeSet::new()))
}

#[cfg(feature = "knowledge-store")]
pub(super) fn visibility_from_row(value: Option<&str>) -> mneme::knowledge::Visibility {
    value
        .and_then(|value| value.parse::<mneme::knowledge::Visibility>().ok())
        .unwrap_or(mneme::knowledge::Visibility::Private)
}
