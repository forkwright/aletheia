//! Integration tests for thesauros's public API surface.
//!
//! WHY: thesauros had zero `crates/thesauros/tests/` integration tests
//! prior to this. The crate loads external domain packs that inject
//! context, tools, and overlays into the agent runtime; its public
//! types (`PackManifest`, `ContextEntry`, `Priority`, `LoadedPack`,
//! `PackSection`, `AgentOverlay`, and the `Error` enum) form the wire
//! contract the bootstrap assembler consumes. Any change to the
//! manifest shape or filter semantics ripples through every agent
//! that uses a domain pack.
//!
//! These tests run against the published API surface only — what
//! nous, aletheia-cli, and the bootstrap pipeline actually consume.
//!
//! Continues the #2814 audit alongside graphe, koina, symbolon,
//! hermeneus, and eidos.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::fs;
use std::path::PathBuf;

use aletheia_thesauros::error::Error;
use aletheia_thesauros::loader::{LoadedPack, load_packs};
use aletheia_thesauros::manifest::{PackManifest, PackPropertyDef, PackToolDef, Priority};
use tempfile::TempDir;

// --- Helpers ---

/// Populate a tempdir with the given files. Parent directories are
/// created as needed; the returned `TempDir` must be kept alive for the
/// life of the test or the files will be cleaned up.
#[expect(
    clippy::disallowed_methods,
    reason = "integration tests write fixture files to a tempdir; pack.toml and context files are loaded from disk so the test must stage them synchronously before calling the pack loader"
)]
fn write_pack(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    for (name, content) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir parent");
        }
        fs::write(&path, content).expect("write file");
    }
    dir
}

fn minimal_manifest_toml() -> &'static str {
    "name = \"mini-pack\"\nversion = \"1.0\"\n"
}

// --- PackManifest serde ---

mod manifest_serde {
    use super::{PackManifest, PackPropertyDef, PackToolDef, Priority};

    #[test]
    fn round_trip_preserves_all_fields() {
        // WHY: the manifest is the source of truth for pack configuration.
        // A round-trip through JSON must preserve every field because this
        // is how the config is persisted, logged, and sent over RPC.
        let json = r#"{
            "name": "acme",
            "version": "2.3.1",
            "description": "an integration-test pack",
            "context": [{
                "path": "ctx/BUSINESS_LOGIC.md",
                "priority": "required",
                "agents": ["analyst"],
                "truncatable": false
            }],
            "tools": [{
                "name": "query",
                "description": "run a query",
                "command": "bin/query.sh",
                "timeout": 45000,
                "input_schema": {
                    "properties": {
                        "sql": {
                            "type": "string",
                            "description": "the query"
                        }
                    },
                    "required": ["sql"]
                }
            }],
            "overlays": {
                "analyst": {
                    "domains": ["healthcare", "sql"]
                }
            }
        }"#;
        let manifest: PackManifest =
            serde_json::from_str(json).expect("deserialize reference manifest");

        // Full round trip through the serializer then back.
        let out = serde_json::to_string(&manifest).expect("serialize");
        let back: PackManifest = serde_json::from_str(&out).expect("deserialize");

        assert_eq!(back.name, "acme");
        assert_eq!(back.version, "2.3.1");
        assert_eq!(back.description.as_deref(), Some("an integration-test pack"));

        let ctx = back.context.first().expect("one context entry");
        assert_eq!(ctx.priority, Priority::Required);
        assert_eq!(ctx.path, "ctx/BUSINESS_LOGIC.md");
        assert_eq!(ctx.agents, vec!["analyst".to_owned()]);
        assert!(!ctx.truncatable);

        let tool = back.tools.first().expect("one tool");
        assert_eq!(tool.name, "query");
        assert_eq!(tool.timeout, 45_000);
        let schema = tool.input_schema.as_ref().expect("input_schema present");
        assert_eq!(schema.required, vec!["sql".to_owned()]);
        let sql_prop = schema.properties.get("sql").expect("sql property present");
        assert_eq!(sql_prop.property_type, "string");

        let analyst_overlay = back.overlays.get("analyst").expect("analyst overlay");
        assert_eq!(
            analyst_overlay.domains,
            vec!["healthcare".to_owned(), "sql".to_owned()]
        );
    }

    #[test]
    fn priority_lowercase_on_wire() {
        // WHY: #[serde(rename_all = "lowercase")] on Priority — the wire
        // form is `"required"`, not `"Required"`. Changing this silently
        // would break every existing pack.toml.
        let priorities = [
            (Priority::Required, r#""required""#),
            (Priority::Important, r#""important""#),
            (Priority::Flexible, r#""flexible""#),
            (Priority::Optional, r#""optional""#),
        ];
        for (prio, expected) in priorities {
            let json = serde_json::to_string(&prio).expect("serialize");
            assert_eq!(json, expected);
            let back: Priority = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, prio);
        }
    }

    #[test]
    fn enum_values_field_omitted_when_none() {
        // WHY: skip_serializing_if = "Option::is_none" — the "enum" key
        // must not appear in serialized JSON when unused, so pack.toml
        // files stay minimal and deserialization of legacy files works.
        let prop_json = r#"{"type": "string", "description": "a string"}"#;
        let prop: PackPropertyDef =
            serde_json::from_str(prop_json).expect("deserialize property");
        assert!(prop.enum_values.is_none());
        let reserialized = serde_json::to_string(&prop).expect("serialize");
        assert!(
            !reserialized.contains("\"enum\""),
            "enum must be skipped: {reserialized}"
        );
        assert!(
            !reserialized.contains("\"default\""),
            "default must be skipped: {reserialized}"
        );
    }

    #[test]
    fn tool_def_default_timeout_on_load() {
        // WHY: pack tool definitions that omit `timeout` must deserialize
        // with the documented default of 30_000 ms (from default_tool_timeout).
        let json = r#"{
            "name": "no_timeout",
            "description": "test",
            "command": "bin/test.sh"
        }"#;
        let tool: PackToolDef = serde_json::from_str(json).expect("deserialize");
        assert_eq!(tool.timeout, 30_000);
        assert!(tool.input_schema.is_none());
    }
}

// --- load_packs public entry point ---

mod load_packs_entry {
    use super::{LoadedPack, PathBuf, Priority, load_packs, minimal_manifest_toml, write_pack};

    #[test]
    fn empty_path_list_returns_empty() {
        // WHY: the function must degrade gracefully with no input,
        // since callers may pass a config-derived Vec that's empty.
        let packs: Vec<LoadedPack> = load_packs(&[]);
        assert!(packs.is_empty());
    }

    #[test]
    fn loads_minimal_pack_through_public_api() {
        // WHY: the happy path end-to-end. A directory with only pack.toml
        // must produce a LoadedPack whose manifest fields match the file.
        let dir = write_pack(&[("pack.toml", minimal_manifest_toml())]);
        let packs = load_packs(&[dir.path().to_path_buf()]);
        assert_eq!(packs.len(), 1);
        let pack = packs.first().expect("one pack");
        assert_eq!(pack.manifest.name, "mini-pack");
        assert_eq!(pack.manifest.version, "1.0");
        assert!(pack.sections.is_empty());
        assert_eq!(pack.root, dir.path());
    }

    #[test]
    fn loads_pack_with_context_sections() {
        // WHY: context files must be resolved, trimmed, and tagged with
        // their manifest priority/truncatable flags. This is the payload
        // the bootstrap assembler actually consumes.
        let toml = r#"
name = "ctx-pack"
version = "0.1.0"

[[context]]
path = "context/BUSINESS_LOGIC.md"
priority = "required"

[[context]]
path = "context/GLOSSARY.md"
priority = "flexible"
truncatable = true
"#;
        let dir = write_pack(&[
            ("pack.toml", toml),
            (
                "context/BUSINESS_LOGIC.md",
                "\n   The invoice total must equal sum(line_items).   \n",
            ),
            ("context/GLOSSARY.md", "ARR = Annual Recurring Revenue"),
        ]);

        let packs = load_packs(&[dir.path().to_path_buf()]);
        let pack = packs.first().expect("one pack");
        assert_eq!(pack.sections.len(), 2);

        let business = pack
            .sections
            .iter()
            .find(|s| s.name == "BUSINESS_LOGIC.md")
            .expect("business logic section");
        // Content is trimmed of surrounding whitespace before storage.
        assert_eq!(
            business.content,
            "The invoice total must equal sum(line_items)."
        );
        assert_eq!(business.priority, Priority::Required);
        assert!(!business.truncatable);
        assert_eq!(business.pack_name, "ctx-pack");

        let glossary = pack
            .sections
            .iter()
            .find(|s| s.name == "GLOSSARY.md")
            .expect("glossary section");
        assert_eq!(glossary.priority, Priority::Flexible);
        assert!(glossary.truncatable);
    }

    #[test]
    fn load_packs_skips_invalid_but_keeps_valid() {
        // WHY: graceful degradation — one broken pack must not prevent
        // loading the rest. The loader warns and skips, returning only
        // successful packs.
        let good = write_pack(&[("pack.toml", minimal_manifest_toml())]);
        let packs = load_packs(&[
            PathBuf::from("/nonexistent/path/to/nothing"),
            good.path().to_path_buf(),
        ]);
        assert_eq!(packs.len(), 1);
        let pack = packs.first().expect("valid pack kept");
        assert_eq!(pack.manifest.name, "mini-pack");
    }

    #[test]
    fn invalid_pack_name_is_skipped_by_load_packs() {
        // WHY: validation is enforced during load, and failures surface
        // as skipped packs via the graceful-degradation path — not as
        // panics or error returns to the caller.
        let dir = write_pack(&[("pack.toml", "name = \"bad name!\"\nversion = \"1.0\"\n")]);
        let packs = load_packs(&[dir.path().to_path_buf()]);
        assert!(
            packs.is_empty(),
            "pack with invalid name must be skipped, got {} packs",
            packs.len()
        );
    }

    #[test]
    fn missing_context_file_does_not_fail_the_pack() {
        // WHY: a single missing context file must not torpedo the whole
        // pack — it's loaded with the surviving sections and a warning.
        let toml = "name = \"partial\"\nversion = \"1.0\"\n\n[[context]]\npath = \"here.md\"\n\n[[context]]\npath = \"gone.md\"\n";
        let dir = write_pack(&[("pack.toml", toml), ("here.md", "present")]);
        let packs = load_packs(&[dir.path().to_path_buf()]);
        let pack = packs.first().expect("pack loaded despite missing section");
        assert_eq!(pack.sections.len(), 1);
        let section = pack.sections.first().expect("surviving section");
        assert_eq!(section.name, "here.md");
    }
}

// --- LoadedPack filtering API ---

mod loaded_pack_filters {
    use super::{load_packs, write_pack};

    fn full_pack_toml() -> &'static str {
        r#"
name = "filter-pack"
version = "1.0"

[[context]]
path = "general.md"

[[context]]
path = "analyst.md"
agents = ["analyst"]

[[context]]
path = "healthcare.md"
agents = ["healthcare"]

[overlays.analyst]
domains = ["healthcare", "sql"]
"#
    }

    #[test]
    fn sections_for_agent_or_domains_matches_by_agent_id() {
        // WHY: when called with an agent id, the filter must return
        // (a) unrestricted sections plus (b) sections whose agents list
        // contains that id — but not sections restricted to a different
        // agent (and not sections gated by an unrelated domain tag).
        let dir = write_pack(&[
            ("pack.toml", full_pack_toml()),
            ("general.md", "general"),
            ("analyst.md", "analyst-only"),
            ("healthcare.md", "healthcare domain"),
        ]);
        let packs = load_packs(&[dir.path().to_path_buf()]);
        let pack = packs.first().expect("loaded");
        let sections = pack.sections_for_agent_or_domains("analyst", &[]);
        let names: Vec<&str> = sections.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"general.md"));
        assert!(names.contains(&"analyst.md"));
        assert!(!names.contains(&"healthcare.md"));
    }

    #[test]
    fn sections_for_agent_or_domains_matches_by_domain_tag() {
        // WHY: the alternative match path — a section tagged with a
        // domain name is visible to any agent whose domain list contains
        // that tag, even if the agent id doesn't match.
        let dir = write_pack(&[
            ("pack.toml", full_pack_toml()),
            ("general.md", "general"),
            ("analyst.md", "analyst"),
            ("healthcare.md", "healthcare"),
        ]);
        let packs = load_packs(&[dir.path().to_path_buf()]);
        let pack = packs.first().expect("loaded");

        // A random agent id with the healthcare domain should see
        // general + healthcare, but not analyst.
        let sections =
            pack.sections_for_agent_or_domains("hermes", &["healthcare".to_owned()]);
        let names: Vec<&str> = sections.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"general.md"));
        assert!(names.contains(&"healthcare.md"));
        assert!(!names.contains(&"analyst.md"));
    }

    #[test]
    fn sections_for_agent_or_domains_unknown_returns_unrestricted_only() {
        // WHY: an agent with no matching id and no matching domains must
        // still receive the unrestricted sections — it's the baseline
        // context every agent sees.
        let dir = write_pack(&[
            ("pack.toml", full_pack_toml()),
            ("general.md", "general"),
            ("analyst.md", "analyst"),
            ("healthcare.md", "healthcare"),
        ]);
        let packs = load_packs(&[dir.path().to_path_buf()]);
        let pack = packs.first().expect("loaded");

        let sections = pack.sections_for_agent_or_domains("unknown", &["other".to_owned()]);
        assert_eq!(sections.len(), 1);
        let only = sections.first().expect("general section");
        assert_eq!(only.name, "general.md");
    }

    #[test]
    fn domains_for_agent_returns_overlay_domains() {
        // WHY: overlays let packs attach domain tags to specific agents.
        // The lookup must return the declared domains in order, or an
        // empty vec if the agent has no overlay.
        let dir = write_pack(&[
            ("pack.toml", full_pack_toml()),
            ("general.md", "general"),
            ("analyst.md", "analyst"),
            ("healthcare.md", "healthcare"),
        ]);
        let packs = load_packs(&[dir.path().to_path_buf()]);
        let pack = packs.first().expect("loaded");

        let analyst_domains = pack.domains_for_agent("analyst");
        assert_eq!(
            analyst_domains,
            vec!["healthcare".to_owned(), "sql".to_owned()]
        );
        assert!(pack.domains_for_agent("nonexistent-agent").is_empty());
    }
}

// --- Error type ---

mod error_type {
    use super::Error;

    #[test]
    fn errors_are_send_sync() {
        // WHY: errors cross task boundaries (background pack loading,
        // warning logs). Losing Send/Sync would break async propagation.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    }

    #[test]
    fn errors_are_debug_and_error_trait_objects() {
        // WHY: the Error type must satisfy std::error::Error so snafu
        // chains work and callers can box it up for dyn dispatch.
        fn assert_error<T: std::error::Error + std::fmt::Debug + 'static>() {}
        assert_error::<Error>();
    }
}
