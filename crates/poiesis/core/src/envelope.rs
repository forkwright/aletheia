//! The deliverable envelope — what every render path consumes and what the
//! QA gate runs over.
//!
//! A [`DeliverableSpec`] carries typed [`Meta`], a [`crate::ids::ThemeId`]
//! (resolved by the theme registry in [[B-002]]), a
//! [`crate::factbase::Factbase`], and a [`Body`]. The body is one of
//! [`crate::bodies::Deck`], [`crate::bodies::DocumentBody`], or
//! [`crate::bodies::Workbook`] — typed sum so the render-side trait can
//! match on body kind without re-deriving it from string tags.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::BodyKind;
use crate::bodies::{Deck, Workbook};
use crate::components::ComponentRegistry;
use crate::error::{MissingMetaFieldSnafu, SpecError, UnknownThemeSnafu};
use crate::factbase::Factbase;
use crate::ids::{ComponentId, ThemeId};

/// Typed metadata required by every deliverable.
///
/// Required fields are enforced at parse time: a [`Meta`] without `title`
/// cannot be constructed. Optional fields are typed as `Option<…>` so the
/// `None` case is explicit at the call site.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    /// Deliverable title; required.
    pub title: String,
    /// Optional author.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Optional creation timestamp; renderers may stamp document properties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Timestamp>,
    /// Optional subject; threaded into PDF/DOCX/PPTX core properties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Optional list of keywords / tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
}

impl Meta {
    /// Construct a [`Meta`] with the required `title` and no optionals.
    ///
    /// # Errors
    ///
    /// Returns [`SpecError::MissingMetaField`] if `title` is empty.
    pub fn new(title: impl Into<String>) -> Result<Self, SpecError> {
        let title = title.into();
        if title.is_empty() {
            return MissingMetaFieldSnafu { field: "title" }.fail();
        }
        Ok(Self {
            title,
            author: None,
            created: None,
            subject: None,
            keywords: Vec::new(),
        })
    }

    /// Validate a deserialised `Meta`, surfacing missing-required failures
    /// even when the value came from JSON / TOML where serde already
    /// admitted the empty case.
    ///
    /// # Errors
    ///
    /// Returns [`SpecError::MissingMetaField`] if `title` is empty.
    pub fn validate(&self) -> Result<(), SpecError> {
        if self.title.is_empty() {
            return MissingMetaFieldSnafu { field: "title" }.fail();
        }
        Ok(())
    }
}

/// The body of a deliverable — a typed sum over the three supported kinds.
// kanon:ignore RUST/non-exhaustive-enum — exhaustive match is part of the
// stable API; new body kinds are an explicit additive evolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Body {
    /// A deck of slides.
    Deck(Deck),
    /// A prose document (wraps the pre-envelope
    /// [`crate::document::Document`]).
    Document(DocumentBodyRepr),
    /// A workbook of sheets.
    Workbook(Workbook),
}

/// Serde-friendly representation of [`DocumentBody`] — the legacy
/// [`crate::document::Document`] does not derive `Serialize` / `Deserialize`
/// today, so the envelope path carries an opaque serializable wrapper.
/// Render-side code consumes [`crate::bodies::DocumentBody`] directly
/// through [`Body::as_document_body`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentBodyRepr {
    /// The document title — duplicated from [`Meta`] for self-containment
    /// at the body level (renderers that only see `Body` still have a
    /// title to thread through).
    pub title: String,
}

impl Body {
    /// The body kind tag — useful for branch-on-kind dispatch.
    #[must_use]
    pub fn kind(&self) -> BodyKind {
        match self {
            Self::Deck(_) => BodyKind::Deck,
            Self::Document(_) => BodyKind::Document,
            Self::Workbook(_) => BodyKind::Workbook,
        }
    }
}

/// The top-level deliverable envelope.
///
/// `DeliverableSpec` is what scaffolding produces, what QA runs over, and
/// what render-side B-NNN entries take as input. The four arms are
/// orthogonal: `meta`, `theme`, `facts`, and `body` are validated
/// independently at the boundary; rejecting a malformed spec is a
/// deterministic walk over these four.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeliverableSpec {
    /// Typed metadata.
    pub meta: Meta,
    /// Theme reference; resolved against the theme registry ([[B-002]]).
    pub theme: ThemeId,
    /// The factbase carrying every cited number.
    pub facts: Factbase,
    /// The body — deck, document, or workbook.
    pub body: Body,
}

impl DeliverableSpec {
    /// Validate the spec at the boundary:
    ///
    /// - `meta` required fields,
    /// - factbase citation graph (cycles, dangling references),
    /// - if `body == Deck`, every `Slide.component` resolves in
    ///   `components` and `Slide.fields` passes the component schema.
    ///
    /// The theme reference is checked against `known_themes` when supplied;
    /// pass an empty slice to skip theme-registry validation (useful for
    /// early scaffolding callers that don't yet have a registry on hand).
    ///
    /// # Errors
    ///
    /// Returns the first [`crate::error::PoiesisError`] the walk
    /// encounters.
    pub fn validate(
        &self,
        components: &ComponentRegistry,
        known_themes: &[ThemeId],
    ) -> Result<(), crate::error::PoiesisError> {
        self.meta.validate()?;
        self.facts.validate()?;
        if !known_themes.is_empty() && !known_themes.contains(&self.theme) {
            return Err(UnknownThemeSnafu {
                theme: self.theme.as_str(),
            }
            .build()
            .into());
        }
        if let Body::Deck(deck) = &self.body {
            for slide in &deck.slides {
                // validate_fields enforces both schema correctness and
                // component-id resolution; we discard the merged payload
                // here, render-side callers re-merge before rendering.
                let _ = components.validate_fields(&slide.component, &slide.fields)?;
            }
        }
        Ok(())
    }

    /// True if `body` matches the expected [`BodyKind`].
    #[must_use]
    pub fn body_kind(&self) -> BodyKind {
        self.body.kind()
    }

    /// Enumerate the component ids the deck body references. Empty for
    /// document and workbook bodies. Useful for the theme coverage lint
    /// and for the agent palette in [[B-010]].
    #[must_use]
    pub fn referenced_components(&self) -> Vec<ComponentId> {
        match &self.body {
            Body::Deck(deck) => {
                let mut ids: Vec<ComponentId> =
                    deck.slides.iter().map(|s| s.component.clone()).collect();
                ids.sort();
                ids.dedup();
                ids
            }
            Body::Document(_) | Body::Workbook(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::bodies::{Slide, Workbook};
    use crate::factbase::{Fact, Source};
    use crate::ids::{ComponentId, FactId, ThemeId};
    use crate::scalar::{AspectRatio, Scalar, Unit};
    use jiff::Timestamp;
    use serde_json::json;

    fn ts() -> Timestamp {
        Timestamp::UNIX_EPOCH
    }

    fn empty_factbase() -> Factbase {
        Factbase::new()
    }

    #[test]
    fn meta_rejects_empty_title() {
        let err = Meta::new("").expect_err("empty title must reject");
        assert!(matches!(
            err,
            SpecError::MissingMetaField { field: "title" }
        ));
    }

    #[test]
    fn meta_validate_catches_post_deserialise_emptiness() {
        let m = Meta {
            title: String::new(),
            author: None,
            created: None,
            subject: None,
            keywords: Vec::new(),
        };
        assert!(m.validate().is_err());
    }

    #[test]
    fn deliverable_workbook_validates_with_empty_factbase() {
        let spec = DeliverableSpec {
            meta: Meta::new("Q1 receipts").unwrap(),
            theme: ThemeId::new("summus").unwrap(),
            facts: empty_factbase(),
            body: Body::Workbook(Workbook { sheets: Vec::new() }),
        };
        spec.validate(&ComponentRegistry::new(), &[])
            .expect("workbook with no facts validates");
        assert_eq!(spec.body_kind(), BodyKind::Workbook);
    }

    #[test]
    fn deliverable_unknown_theme_rejects_when_registry_supplied() {
        let spec = DeliverableSpec {
            meta: Meta::new("Untitled").unwrap(),
            theme: ThemeId::new("missing").unwrap(),
            facts: empty_factbase(),
            body: Body::Document(DocumentBodyRepr {
                title: "Untitled".to_owned(),
            }),
        };
        let known = vec![ThemeId::new("summus").unwrap()];
        let err = spec
            .validate(&ComponentRegistry::new(), &known)
            .expect_err("unknown theme rejects");
        match err {
            crate::error::PoiesisError::Spec {
                source: SpecError::UnknownTheme { theme },
            } => {
                assert_eq!(theme, "missing");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn deliverable_deck_with_unknown_component_rejects() {
        let spec = DeliverableSpec {
            meta: Meta::new("Pitch").unwrap(),
            theme: ThemeId::new("summus").unwrap(),
            facts: empty_factbase(),
            body: Body::Deck(Deck {
                aspect: AspectRatio::WIDESCREEN_16_9,
                slides: vec![Slide {
                    component: ComponentId::new("ghost").unwrap(),
                    fields: json!({}),
                    notes: None,
                }],
            }),
        };
        let err = spec
            .validate(&ComponentRegistry::new(), &[])
            .expect_err("unknown component rejects");
        match err {
            crate::error::PoiesisError::Registry {
                source: crate::error::RegistryError::UnknownComponent { component },
            } => {
                assert_eq!(component, "ghost");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn deliverable_facts_cycle_propagates() {
        let mut facts = Factbase::new();
        facts.add_fact(Fact {
            id: FactId::new("a").unwrap(),
            value: Scalar::Count { value: 0 },
            unit: Unit::Count,
            source: Source::Reference {
                fact: FactId::new("b").unwrap(),
            },
            captured: ts(),
        });
        facts.add_fact(Fact {
            id: FactId::new("b").unwrap(),
            value: Scalar::Count { value: 0 },
            unit: Unit::Count,
            source: Source::Reference {
                fact: FactId::new("a").unwrap(),
            },
            captured: ts(),
        });
        let spec = DeliverableSpec {
            meta: Meta::new("cycle test").unwrap(),
            theme: ThemeId::new("summus").unwrap(),
            facts,
            body: Body::Workbook(Workbook { sheets: Vec::new() }),
        };
        let err = spec
            .validate(&ComponentRegistry::new(), &[])
            .expect_err("cycle");
        assert!(matches!(
            err,
            crate::error::PoiesisError::Factbase {
                source: crate::error::FactbaseError::Cycle { .. },
            }
        ));
    }

    #[test]
    fn round_trip_via_serde_preserves_envelope() {
        let spec = DeliverableSpec {
            meta: Meta::new("Roundtrip").unwrap(),
            theme: ThemeId::new("summus").unwrap(),
            facts: empty_factbase(),
            body: Body::Workbook(Workbook { sheets: Vec::new() }),
        };
        let s = serde_json::to_string(&spec).unwrap();
        let back: DeliverableSpec = serde_json::from_str(&s).unwrap();
        assert_eq!(back, spec);
    }

    #[test]
    fn referenced_components_lists_used_ids_sorted_and_deduped() {
        let spec = DeliverableSpec {
            meta: Meta::new("Pitch").unwrap(),
            theme: ThemeId::new("summus").unwrap(),
            facts: empty_factbase(),
            body: Body::Deck(Deck {
                aspect: AspectRatio::WIDESCREEN_16_9,
                slides: vec![
                    Slide {
                        component: ComponentId::new("title").unwrap(),
                        fields: json!({}),
                        notes: None,
                    },
                    Slide {
                        component: ComponentId::new("statement").unwrap(),
                        fields: json!({}),
                        notes: None,
                    },
                    Slide {
                        component: ComponentId::new("title").unwrap(),
                        fields: json!({}),
                        notes: None,
                    },
                ],
            }),
        };
        let ids = spec.referenced_components();
        assert_eq!(
            ids,
            vec![
                ComponentId::new("statement").unwrap(),
                ComponentId::new("title").unwrap(),
            ]
        );
    }
}
