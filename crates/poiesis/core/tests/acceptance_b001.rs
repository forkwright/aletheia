//! B-001 acceptance tests.
//!
//! Mirrors the five acceptance criteria from the planning entry:
//!
//! 1. Round-trip a valid spec.
//! 2. Reject planted-bad specs with precise (JSON-pointer) errors.
//! 3. Drop-a-pack works (filesystem discovery + slot validation without core
//!    recompile).
//! 4. Factbase resolves; the citation graph is DAG-checked.
//! 5. Current `organon`-shaped `Document` → `Renderer` path compiles and runs
//!    unchanged.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "integration tests can panic-on-fail; serde_json::Value indexing is the natural access pattern"
)]

use std::fs;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use poiesis_core::{
    AspectRatio, Block, Body, Claim, ClaimId, ComponentId, ComponentRegistry, DataSourceRegistry,
    Deck, DeliverableSpec, Document, DocumentBody, Expr, Fact, FactId, Factbase, Location, Meta,
    Metadata, Money, Renderer, RichText, Scalar, ScalarKind, Sheet, SheetName, Slide, Source, Span,
    ThemeId, Tolerance, Unit, Workbook, WorkbookCell,
};
use serde_json::json;

// =========================================================================
// Acceptance #1 — round-trip a valid spec for each body kind.
// =========================================================================

#[test]
fn acceptance_round_trip_deck_spec() {
    let spec = DeliverableSpec {
        meta: Meta::new("offsite-2026-q1").expect("title"),
        theme: ThemeId::new("summus").expect("theme id"),
        facts: empty_factbase(),
        body: Body::Deck(Deck {
            aspect: AspectRatio::WIDESCREEN_16_9,
            slides: vec![Slide {
                component: ComponentId::new("title").expect("component id"),
                fields: json!({"text": "Welcome"}),
                notes: None,
            }],
        }),
    };
    let json_text = serde_json::to_string(&spec).expect("ser");
    let back: DeliverableSpec = serde_json::from_str(&json_text).expect("de");
    assert_eq!(back, spec);
}

#[test]
fn acceptance_round_trip_workbook_spec() {
    let spec = DeliverableSpec {
        meta: Meta::new("Q1 ledger").expect("title"),
        theme: ThemeId::new("summus").expect("theme id"),
        facts: empty_factbase(),
        body: Body::Workbook(Workbook {
            sheets: vec![Sheet {
                name: SheetName::new("Receipts").expect("sheet name"),
                headers: vec!["Description".to_owned(), "Amount".to_owned()],
                rows: vec![vec![
                    WorkbookCell::Lit {
                        value: Scalar::Text {
                            value: "Software".to_owned(),
                        },
                    },
                    WorkbookCell::Lit {
                        value: Scalar::Money {
                            value: Money::from_units(150).expect("range"),
                        },
                    },
                ]],
                column_types: vec![ScalarKind::Text, ScalarKind::Money],
            }],
        }),
    };
    let json_text = serde_json::to_string(&spec).expect("ser");
    let back: DeliverableSpec = serde_json::from_str(&json_text).expect("de");
    assert_eq!(back, spec);
}

#[test]
fn acceptance_round_trip_document_spec() {
    let spec = DeliverableSpec {
        meta: Meta::new("README").expect("title"),
        theme: ThemeId::new("summus").expect("theme id"),
        facts: empty_factbase(),
        body: Body::Document(poiesis_core::envelope::DocumentBodyRepr {
            title: "README".to_owned(),
        }),
    };
    let json_text = serde_json::to_string(&spec).expect("ser");
    let back: DeliverableSpec = serde_json::from_str(&json_text).expect("de");
    assert_eq!(back, spec);
}

// =========================================================================
// Acceptance #2 — reject planted-bad specs with precise errors.
// =========================================================================

#[test]
fn acceptance_reject_missing_required_meta_field() {
    let m = Meta {
        title: String::new(),
        author: None,
        created: None,
        subject: None,
        keywords: Vec::new(),
    };
    let err = m.validate().expect_err("empty title rejects");
    match err {
        poiesis_core::SpecError::MissingMetaField { field } => assert_eq!(field, "title"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn acceptance_reject_unknown_component_in_deck() {
    let spec = DeliverableSpec {
        meta: Meta::new("pitch").expect("title"),
        theme: ThemeId::new("summus").expect("theme id"),
        facts: empty_factbase(),
        body: Body::Deck(Deck {
            aspect: AspectRatio::WIDESCREEN_16_9,
            slides: vec![Slide {
                component: ComponentId::new("ghost").expect("id"),
                fields: json!({}),
                notes: None,
            }],
        }),
    };
    let err = spec
        .validate(&ComponentRegistry::new(), &[])
        .expect_err("unknown component rejects");
    match err {
        poiesis_core::PoiesisError::Registry {
            source: poiesis_core::RegistryError::UnknownComponent { component },
        } => assert_eq!(component, "ghost"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn acceptance_reject_bad_slot_type_with_json_pointer() {
    let tmp = TestTempDir::new();
    write_pack(
        tmp.path(),
        "hero",
        r#"{"type":"object","required":["count"],"properties":{"count":{"type":"integer"}}}"#,
        "[ooxml]\nkind = \"hero\"\n",
    );
    let mut registry = ComponentRegistry::new();
    registry.discover(tmp.path()).expect("discover");
    let spec = DeliverableSpec {
        meta: Meta::new("pitch").expect("title"),
        theme: ThemeId::new("summus").expect("theme id"),
        facts: empty_factbase(),
        body: Body::Deck(Deck {
            aspect: AspectRatio::WIDESCREEN_16_9,
            slides: vec![Slide {
                component: ComponentId::new("hero").expect("id"),
                fields: json!({"count": "three"}),
                notes: None,
            }],
        }),
    };
    let err = spec
        .validate(&registry, &[])
        .expect_err("bad slot type rejects");
    match err {
        poiesis_core::PoiesisError::Registry {
            source: poiesis_core::RegistryError::SlotValidation { pointer, detail },
        } => {
            assert_eq!(pointer, "/count");
            assert!(detail.contains("integer"));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn acceptance_reject_unknown_theme_when_registry_supplied() {
    let spec = DeliverableSpec {
        meta: Meta::new("x").expect("title"),
        theme: ThemeId::new("missing").expect("theme id"),
        facts: empty_factbase(),
        body: Body::Document(poiesis_core::envelope::DocumentBodyRepr {
            title: "x".to_owned(),
        }),
    };
    let known = vec![ThemeId::new("summus").expect("theme id")];
    let err = spec
        .validate(&ComponentRegistry::new(), &known)
        .expect_err("unknown theme rejects");
    assert!(matches!(
        err,
        poiesis_core::PoiesisError::Spec {
            source: poiesis_core::SpecError::UnknownTheme { .. }
        }
    ));
}

#[test]
fn acceptance_reject_citation_cycle_with_path() {
    let mut facts = Factbase::new();
    facts.add_fact(Fact {
        id: FactId::new("a").expect("id"),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Reference {
            fact: FactId::new("b").expect("id"),
        },
        captured: Timestamp::UNIX_EPOCH,
    });
    facts.add_fact(Fact {
        id: FactId::new("b").expect("id"),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Reference {
            fact: FactId::new("a").expect("id"),
        },
        captured: Timestamp::UNIX_EPOCH,
    });
    let spec = DeliverableSpec {
        meta: Meta::new("cycle").expect("title"),
        theme: ThemeId::new("summus").expect("theme id"),
        facts,
        body: Body::Workbook(Workbook { sheets: Vec::new() }),
    };
    let err = spec
        .validate(&ComponentRegistry::new(), &[])
        .expect_err("cycle");
    let path = match err {
        poiesis_core::PoiesisError::Factbase {
            source: poiesis_core::FactbaseError::Cycle { path },
        } => path,
        other => panic!("expected Cycle, got {other:?}"),
    };
    assert!(path.contains(&"a".to_owned()));
    assert!(path.contains(&"b".to_owned()));
}

#[test]
fn acceptance_reject_unsourced_claim() {
    let mut facts = Factbase::new();
    facts.add_claim(Claim {
        id: ClaimId::new("c1").expect("id"),
        text: "x is 7".to_owned(),
        asserts: FactId::new("absent").expect("id"),
        location: Location {
            at: "deck/slide/1".to_owned(),
        },
        tolerance: Tolerance::STRICT,
    });
    let err = facts.validate().expect_err("unsourced claim rejects");
    assert!(matches!(
        err,
        poiesis_core::FactbaseError::UnknownFact { ref id, .. } if id == "absent"
    ));
}

// =========================================================================
// Acceptance #3 — drop-a-pack works.
// =========================================================================

#[test]
fn acceptance_drop_a_pack_discovers_validates_and_lists() {
    let tmp = TestTempDir::new();
    write_pack(
        tmp.path(),
        "title",
        r#"{"type":"object","required":["text"],"properties":{"text":{"type":"string","minLength":1}}}"#,
        "[ooxml]\nkind = \"title\"\n",
    );
    write_pack(
        tmp.path(),
        "statement",
        r#"{"type":"object","required":["body"],"properties":{"body":{"type":"string"}}}"#,
        "[ooxml]\nkind = \"statement\"\n",
    );

    let mut registry = ComponentRegistry::new();
    let n = registry.discover(tmp.path()).expect("discover succeeds");
    assert_eq!(n, 2);

    let listed = registry.list_components();
    assert_eq!(
        listed,
        vec![
            ComponentId::new("statement").expect("id"),
            ComponentId::new("title").expect("id"),
        ]
    );

    let title_id = ComponentId::new("title").expect("id");
    let merged = registry
        .validate_fields(&title_id, &json!({"text": "Welcome"}))
        .expect("valid fields");
    assert_eq!(merged["text"], "Welcome");

    let err = registry
        .validate_fields(&title_id, &json!({}))
        .expect_err("missing required rejects");
    assert!(matches!(
        err,
        poiesis_core::RegistryError::SlotValidation { ref pointer, .. } if pointer == "/text"
    ));
}

// =========================================================================
// Acceptance #4 — factbase resolves, with cycle detection.
// =========================================================================

#[test]
fn acceptance_factbase_resolves_in_declaration_order() {
    let mut facts = Factbase::new();
    facts.add_fact(manual("a", Scalar::Count { value: 10 }, Unit::Count));
    facts.add_fact(manual("b", Scalar::Count { value: 32 }, Unit::Count));
    facts.add_fact(Fact {
        id: FactId::new("total").expect("id"),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Derived {
            formula: Expr::Add {
                a: FactId::new("a").expect("id"),
                b: FactId::new("b").expect("id"),
            },
            inputs: vec![FactId::new("a").expect("id"), FactId::new("b").expect("id")],
        },
        captured: Timestamp::UNIX_EPOCH,
    });
    facts.add_fact(Fact {
        id: FactId::new("alias_total").expect("id"),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Reference {
            fact: FactId::new("total").expect("id"),
        },
        captured: Timestamp::UNIX_EPOCH,
    });

    let resolved = facts.resolve(&DataSourceRegistry::new()).expect("resolves");
    let total = resolved
        .get(&FactId::new("total").expect("id"))
        .expect("total present");
    assert_eq!(total.value, Scalar::Count { value: 42 });
    let alias = resolved
        .get(&FactId::new("alias_total").expect("id"))
        .expect("alias present");
    assert_eq!(alias.value, Scalar::Count { value: 42 });
}

#[test]
fn acceptance_factbase_resolves_money_and_ratio() {
    let mut facts = Factbase::new();
    facts.add_fact(manual(
        "revenue",
        Scalar::Money {
            value: Money::from_units(1000).expect("range"),
        },
        Unit::Usd,
    ));
    facts.add_fact(manual(
        "cost",
        Scalar::Money {
            value: Money::from_units(600).expect("range"),
        },
        Unit::Usd,
    ));
    facts.add_fact(Fact {
        id: FactId::new("profit").expect("id"),
        value: Scalar::Money {
            value: Money::from_micros(0),
        },
        unit: Unit::Usd,
        source: Source::Derived {
            formula: Expr::Sub {
                a: FactId::new("revenue").expect("id"),
                b: FactId::new("cost").expect("id"),
            },
            inputs: vec![
                FactId::new("revenue").expect("id"),
                FactId::new("cost").expect("id"),
            ],
        },
        captured: Timestamp::UNIX_EPOCH,
    });
    facts.add_fact(Fact {
        id: FactId::new("margin").expect("id"),
        value: Scalar::Ratio { value: 0.0 },
        unit: Unit::Percent,
        source: Source::Derived {
            formula: Expr::Div {
                a: FactId::new("profit").expect("id"),
                b: FactId::new("revenue").expect("id"),
            },
            inputs: vec![
                FactId::new("profit").expect("id"),
                FactId::new("revenue").expect("id"),
            ],
        },
        captured: Timestamp::UNIX_EPOCH,
    });

    let resolved = facts.resolve(&DataSourceRegistry::new()).expect("resolves");
    let profit = resolved
        .get(&FactId::new("profit").expect("id"))
        .expect("profit present");
    assert_eq!(
        profit.value,
        Scalar::Money {
            value: Money::from_units(400).expect("range"),
        }
    );
    let margin = resolved
        .get(&FactId::new("margin").expect("id"))
        .expect("margin present");
    let Scalar::Ratio { value: m } = margin.value else {
        panic!("margin is not a ratio");
    };
    assert!((m - 0.4).abs() < 1e-9);
}

#[test]
fn acceptance_factbase_skips_sql_when_no_adapter_configured() {
    // A factbase with NO Sql facts must resolve cleanly without any
    // DataSource adapter (the "no data deps for a deck with no Sql facts"
    // contract).
    let mut facts = Factbase::new();
    facts.add_fact(manual("only", Scalar::Count { value: 1 }, Unit::Count));
    let resolved = facts
        .resolve(&DataSourceRegistry::new())
        .expect("no-Sql factbase resolves with empty registry");
    assert_eq!(resolved.len(), 1);
}

// =========================================================================
// Acceptance #5 — legacy organon → Document → Renderer path still works.
// =========================================================================

/// A stand-in for organon's render path: take a `Document`, produce bytes
/// via a `Renderer`. This compiles iff the pre-envelope surface remains
/// unchanged, which is the contract we promised.
struct PlaintextRenderer;

impl Renderer for PlaintextRenderer {
    type Error = std::io::Error;

    fn render(&self, doc: &Document) -> Result<Vec<u8>, Self::Error> {
        let mut buf = String::new();
        buf.push_str(&doc.metadata.title);
        buf.push('\n');
        for block in &doc.content {
            match block {
                Block::Heading { text, .. } | Block::Paragraph(text) => {
                    buf.push_str(&text.plain_text());
                    buf.push('\n');
                }
                Block::Note(note) => {
                    buf.push_str(note.kind.label());
                    buf.push_str(": ");
                    buf.push_str(&note.body.plain_text());
                    buf.push('\n');
                }
                Block::DisplayMath(expr) | Block::RawBlock { content: expr, .. } => {
                    buf.push_str(expr);
                    buf.push('\n');
                }
                Block::Table(_) | Block::List { .. } | Block::Image(_) | Block::PageBreak => {}
            }
        }
        Ok(buf.into_bytes())
    }

    fn format(&self) -> &'static str {
        "txt"
    }
}

#[test]
fn acceptance_legacy_renderer_path_compiles_and_runs() {
    let doc = Document {
        metadata: Metadata {
            title: "Hello".to_owned(),
            author: Some("Operator".to_owned()),
            created: None,
        },
        content: vec![
            Block::Heading {
                level: 1,
                text: RichText {
                    spans: vec![Span::Plain("World".to_owned())],
                },
            },
            Block::Paragraph(RichText {
                spans: vec![Span::Plain("Hello, world.".to_owned())],
            }),
        ],
    };
    let renderer = PlaintextRenderer;
    let bytes = renderer.render(&doc).expect("renders");
    let text = String::from_utf8(bytes).expect("utf8");
    assert!(text.starts_with("Hello\nWorld\nHello, world."));
    assert_eq!(renderer.format(), "txt");
}

#[test]
fn acceptance_legacy_document_round_trips_through_envelope() {
    // Wrap a legacy Document inside the new envelope's Body::Document arm
    // (via DocumentBody). The Document itself is preserved verbatim.
    let doc = Document::new("Hello");
    let body = DocumentBody::new(doc);
    assert_eq!(body.document.metadata.title, "Hello");
}

// =========================================================================
// Helpers.
// =========================================================================

fn empty_factbase() -> Factbase {
    Factbase::new()
}

fn manual(id: &str, value: Scalar, unit: Unit) -> Fact {
    Fact {
        id: FactId::new(id).expect("id"),
        value,
        unit,
        source: Source::Manual {
            note: "test".to_owned(),
            captured_by: "tester".to_owned(),
        },
        captured: Timestamp::UNIX_EPOCH,
    }
}

fn write_pack(root: &Path, id: &str, schema: &str, recipe: &str) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).expect("mkdir");
    fs::write(dir.join("schema.json"), schema).expect("write schema");
    fs::write(dir.join("recipe.toml"), recipe).expect("write recipe");
    fs::write(dir.join("template.html.j2"), "<div></div>").expect("write template");
}

/// Scoped tempdir helper; avoids pulling tempfile.
struct TestTempDir {
    path: PathBuf,
}

impl TestTempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("poiesis-acceptance-{pid}-{n}"));
        fs::create_dir_all(&path).expect("mkdir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
