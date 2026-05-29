//! The open component registry.
//!
//! Components are discovered from a filesystem root (default
//! `crates/poiesis/components/<id>/` per the architecture contract); each
//! pack is a directory containing:
//!
//! | file              | required | purpose                                                    |
//! |-------------------|----------|------------------------------------------------------------|
//! | `schema.json`     | yes      | JSON schema validating the `Slide.fields` payload          |
//! | `recipe.toml`     | yes      | declarative OOXML recipe ([[B-004]] consumes)               |
//! | `template.html.j2`| yes      | minijinja template ([[B-003]] consumes)                     |
//! | `defaults.json`   | no       | smart defaults merged before schema validation             |
//! | `tokens.toml`     | no       | theme tokens this component reads (drives coverage lint)    |
//!
//! Discovery returns a [`ComponentRegistry`] whose
//! [`ComponentRegistry::validate_fields`] is what [`crate::envelope`] calls
//! to reject planted-bad `Slide.fields` payloads at the boundary.
//!
//! **The JSON-schema validator shipped here is intentionally minimal.** It
//! enforces the subset of Draft-7 that the v1.0.0 component packs need:
//! `type` (object/array/string/number/integer/boolean/null),
//! `required` array, `properties` map, `items` schema, `enum` allow-list,
//! `minLength` / `maxLength`. The richer surface (regex `pattern`,
//! `$ref`, `oneOf`, `allOf`) is deferred to a vendored validator the
//! moment a component pack needs it; this keeps the dep tree narrow.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{
    MalformedRecipeSnafu, MalformedSchemaSnafu, MissingPackFileSnafu, RegistryError,
    SlotValidationSnafu,
};
use crate::ids::ComponentId;

const PACK_SCHEMA: &str = "schema.json";
const PACK_RECIPE: &str = "recipe.toml";
const PACK_TEMPLATE: &str = "template.html.j2";
const PACK_DEFAULTS: &str = "defaults.json";
const PACK_TOKENS: &str = "tokens.toml";

/// A theme token name a component consumes. Free-form by design; the theme
/// registry ([[B-002]]) is responsible for resolving the token.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TokenRef(pub String);

impl TokenRef {
    /// Wrap a string as a token reference.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

/// A discovered component pack.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentDef {
    /// The component id; matches the pack directory name.
    pub id: ComponentId,
    /// The JSON-schema document validating `Slide.fields` payloads.
    pub schema: Value,
    /// Default values merged into `Slide.fields` before validation.
    pub defaults: Value,
    /// Filesystem path of the HTML template (consumed by [[B-003]]).
    pub html: PathBuf,
    /// Filesystem path of the OOXML recipe (consumed by [[B-004]]).
    pub ooxml: PathBuf,
    /// Theme tokens this component reads.
    pub tokens: Vec<TokenRef>,
}

/// The registered set of components, keyed by [`ComponentId`].
#[derive(Debug, Clone, Default)]
pub struct ComponentRegistry {
    by_id: BTreeMap<ComponentId, ComponentDef>,
}

impl ComponentRegistry {
    /// Construct an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a [`ComponentDef`]; replaces any prior definition for the
    /// same id.
    pub fn insert(&mut self, def: ComponentDef) {
        self.by_id.insert(def.id.clone(), def);
    }

    /// Look up a component by id.
    #[must_use]
    pub fn get(&self, id: &ComponentId) -> Option<&ComponentDef> {
        self.by_id.get(id)
    }

    /// Iterate every registered component in id-sorted order.
    pub fn iter(&self) -> impl Iterator<Item = &ComponentDef> {
        self.by_id.values()
    }

    /// Enumerate every registered component id in sorted order — the
    /// agent-facing palette.
    #[must_use]
    pub fn list_components(&self) -> Vec<ComponentId> {
        self.by_id.keys().cloned().collect()
    }

    /// Number of registered components.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// True if no components are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Discover every valid component pack under `root` and merge it into
    /// this registry. Returns the number of components newly registered.
    ///
    /// A pack directory whose name fails [`ComponentId`] validation is
    /// skipped (it cannot be a valid component id). A pack missing a
    /// required file or carrying a malformed schema or recipe returns an
    /// error and the discovery aborts — better-loud than silently-partial.
    ///
    /// # Errors
    ///
    /// Returns the first [`RegistryError`] encountered during discovery.
    pub fn discover(&mut self, root: &Path) -> Result<usize, RegistryError> {
        if !root.exists() {
            // An absent root is a no-op, not an error; consumers may ship
            // with no packs and add them later via [`Self::insert`].
            return Ok(0);
        }
        let entries = fs::read_dir(root).map_err(|e| RegistryError::Io {
            path: root.display().to_string(),
            detail: e.to_string(),
        })?;
        let mut newly = 0usize;
        let mut dirs: Vec<PathBuf> = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| RegistryError::Io {
                path: root.display().to_string(),
                detail: e.to_string(),
            })?;
            let path = entry.path();
            let ft = entry.file_type().map_err(|e| RegistryError::Io {
                path: path.display().to_string(),
                detail: e.to_string(),
            })?;
            if ft.is_dir() {
                dirs.push(path);
            }
        }
        dirs.sort();
        for dir in dirs {
            let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Ok(id) = ComponentId::new(name) else {
                continue;
            };
            let def = load_pack(id, &dir)?;
            self.insert(def);
            newly += 1;
        }
        Ok(newly)
    }

    /// Validate a `Slide.fields` payload against the component's schema.
    ///
    /// On success returns the payload with defaults merged in. On failure
    /// returns a [`RegistryError::SlotValidation`] whose `pointer` is a
    /// JSON-pointer (RFC 6901) into the payload.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::UnknownComponent`] if `id` is not registered
    /// and [`RegistryError::SlotValidation`] if the schema rejects the
    /// merged payload.
    pub fn validate_fields(
        &self,
        id: &ComponentId,
        fields: &Value,
    ) -> Result<Value, RegistryError> {
        let def = self
            .get(id)
            .ok_or_else(|| RegistryError::UnknownComponent {
                component: id.as_str().to_owned(),
            })?;
        let merged = merge_defaults(&def.defaults, fields);
        let mut errors: Vec<SchemaIssue> = Vec::new();
        validate_against_schema(&def.schema, &merged, "", &mut errors);
        if let Some(first) = errors.into_iter().next() {
            return SlotValidationSnafu {
                pointer: first.pointer,
                detail: first.detail,
            }
            .fail();
        }
        Ok(merged)
    }
}

fn load_pack(id: ComponentId, dir: &Path) -> Result<ComponentDef, RegistryError> {
    let schema_path = dir.join(PACK_SCHEMA);
    let recipe_path = dir.join(PACK_RECIPE);
    let template_path = dir.join(PACK_TEMPLATE);
    if !schema_path.exists() {
        return MissingPackFileSnafu {
            component: id.as_str(),
            file: PACK_SCHEMA,
        }
        .fail();
    }
    if !recipe_path.exists() {
        return MissingPackFileSnafu {
            component: id.as_str(),
            file: PACK_RECIPE,
        }
        .fail();
    }
    if !template_path.exists() {
        return MissingPackFileSnafu {
            component: id.as_str(),
            file: PACK_TEMPLATE,
        }
        .fail();
    }

    let schema_text = fs::read_to_string(&schema_path).map_err(|e| RegistryError::Io {
        path: schema_path.display().to_string(),
        detail: e.to_string(),
    })?;
    let schema: Value = serde_json::from_str(&schema_text).map_err(|e| {
        MalformedSchemaSnafu {
            component: id.as_str(),
            detail: e.to_string(),
        }
        .build()
    })?;

    let recipe_text = fs::read_to_string(&recipe_path).map_err(|e| RegistryError::Io {
        path: recipe_path.display().to_string(),
        detail: e.to_string(),
    })?;
    // Recipe parse failure should be reported; we only need to detect parse
    // errors here — the structured recipe types belong to [[B-004]].
    let _recipe: toml::Value = toml::from_str(&recipe_text).map_err(|e| {
        MalformedRecipeSnafu {
            component: id.as_str(),
            detail: e.to_string(),
        }
        .build()
    })?;

    let defaults = load_optional_json(&dir.join(PACK_DEFAULTS), id.as_str())?;
    let tokens = load_optional_tokens(&dir.join(PACK_TOKENS), id.as_str())?;

    Ok(ComponentDef {
        id,
        schema,
        defaults,
        html: template_path,
        ooxml: recipe_path,
        tokens,
    })
}

fn load_optional_json(path: &Path, component: &str) -> Result<Value, RegistryError> {
    if !path.exists() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    let text = fs::read_to_string(path).map_err(|e| RegistryError::Io {
        path: path.display().to_string(),
        detail: e.to_string(),
    })?;
    serde_json::from_str(&text).map_err(|e| {
        MalformedSchemaSnafu {
            component,
            detail: e.to_string(),
        }
        .build()
    })
}

#[derive(Deserialize)]
struct TokensManifest {
    #[serde(default)]
    tokens: Vec<String>,
}

fn load_optional_tokens(path: &Path, component: &str) -> Result<Vec<TokenRef>, RegistryError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|e| RegistryError::Io {
        path: path.display().to_string(),
        detail: e.to_string(),
    })?;
    let parsed: TokensManifest = toml::from_str(&text).map_err(|e| {
        MalformedRecipeSnafu {
            component,
            detail: e.to_string(),
        }
        .build()
    })?;
    Ok(parsed.tokens.into_iter().map(TokenRef::new).collect())
}

/// Merge `defaults` into `fields`: for an object, missing keys are populated
/// from defaults; for any other shape, `fields` wins outright. This is a
/// deliberately shallow merge — JSON-Schema layered defaults are not in
/// scope at the v1.0.0 boundary.
fn merge_defaults(defaults: &Value, fields: &Value) -> Value {
    match (defaults, fields) {
        (Value::Object(d), Value::Object(f)) => {
            let mut out = serde_json::Map::with_capacity(d.len() + f.len());
            for (k, v) in d {
                out.insert(k.clone(), v.clone());
            }
            for (k, v) in f {
                out.insert(k.clone(), v.clone());
            }
            Value::Object(out)
        }
        _ => fields.clone(),
    }
}

#[derive(Debug, Clone)]
struct SchemaIssue {
    pointer: String,
    detail: String,
}

/// Run the minimal JSON-schema check. Walks `schema` against `value` and
/// pushes any issues found into `errors`. Mutating-out so the caller can
/// pick the first issue (parse-don't-validate prefers fail-fast surfacing).
fn validate_against_schema(
    schema: &Value,
    value: &Value,
    pointer: &str,
    errors: &mut Vec<SchemaIssue>,
) {
    let Value::Object(schema_obj) = schema else {
        // A non-object schema is treated as "any" — useful for `true` /
        // empty-object schemas during early authoring.
        return;
    };

    if let Some(Value::String(expected)) = schema_obj.get("type")
        && !value_matches_type(value, expected)
    {
        errors.push(SchemaIssue {
            pointer: pointer.to_owned(),
            detail: format!("expected type {expected:?}, got {}", value_type_name(value)),
        });
        return;
    }

    if let Some(Value::Array(allowed)) = schema_obj.get("enum")
        && !allowed.contains(value)
    {
        errors.push(SchemaIssue {
            pointer: pointer.to_owned(),
            detail: format!("value not in enum (allowed: {} options)", allowed.len()),
        });
        return;
    }

    match value {
        Value::Object(obj) => {
            if let Some(Value::Array(required)) = schema_obj.get("required") {
                for req in required {
                    if let Value::String(req_name) = req
                        && !obj.contains_key(req_name)
                    {
                        errors.push(SchemaIssue {
                            pointer: format!("{pointer}/{req_name}"),
                            detail: "required property missing".to_owned(),
                        });
                    }
                }
            }
            if let Some(Value::Object(props)) = schema_obj.get("properties") {
                for (key, sub_schema) in props {
                    if let Some(sub_value) = obj.get(key) {
                        let sub_pointer = format!("{pointer}/{key}");
                        validate_against_schema(sub_schema, sub_value, &sub_pointer, errors);
                    }
                }
            }
        }
        Value::Array(items) => {
            if let Some(item_schema) = schema_obj.get("items") {
                for (i, item) in items.iter().enumerate() {
                    let sub_pointer = format!("{pointer}/{i}");
                    validate_against_schema(item_schema, item, &sub_pointer, errors);
                }
            }
        }
        Value::String(s) => {
            let len = s.chars().count();
            if let Some(min) = schema_obj.get("minLength").and_then(Value::as_u64)
                && let Ok(min_us) = usize::try_from(min)
                && len < min_us
            {
                errors.push(SchemaIssue {
                    pointer: pointer.to_owned(),
                    detail: format!("string shorter than minLength {min}"),
                });
            }
            if let Some(max) = schema_obj.get("maxLength").and_then(Value::as_u64)
                && let Ok(max_us) = usize::try_from(max)
                && len > max_us
            {
                errors.push(SchemaIssue {
                    pointer: pointer.to_owned(),
                    detail: format!("string longer than maxLength {max}"),
                });
            }
        }
        _ => {}
    }
}

fn value_matches_type(value: &Value, expected: &str) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    }
}

fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
        Value::Null => "null",
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test assertions over serde_json::Value require indexing"
)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn write_pack(root: &Path, id: &str, schema: &str, recipe: &str) {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(PACK_SCHEMA), schema).unwrap();
        fs::write(dir.join(PACK_RECIPE), recipe).unwrap();
        let mut tpl = fs::File::create(dir.join(PACK_TEMPLATE)).unwrap();
        writeln!(tpl, "<div></div>").unwrap();
    }

    #[test]
    fn discovery_picks_up_valid_packs() {
        let tmp = tempdir();
        let root = tmp.path();
        write_pack(
            root,
            "title",
            r#"{"type":"object","required":["text"],"properties":{"text":{"type":"string"}}}"#,
            "[ooxml]\nkind = \"title\"\n",
        );
        write_pack(
            root,
            "stat_cards",
            r#"{"type":"object"}"#,
            "[ooxml]\nkind = \"stat_cards\"\n",
        );
        let mut registry = ComponentRegistry::new();
        let n = registry.discover(root).expect("discovery succeeds");
        assert_eq!(n, 2);
        assert_eq!(registry.len(), 2);
        let listed = registry.list_components();
        assert_eq!(
            listed,
            vec![
                ComponentId::new("stat_cards").unwrap(),
                ComponentId::new("title").unwrap(),
            ]
        );
    }

    #[test]
    fn discovery_absent_root_is_no_op() {
        let tmp = tempdir();
        let missing = tmp.path().join("does_not_exist");
        let mut registry = ComponentRegistry::new();
        let n = registry.discover(&missing).expect("no-op");
        assert_eq!(n, 0);
        assert!(registry.is_empty());
    }

    #[test]
    fn discovery_rejects_pack_missing_required_file() {
        let tmp = tempdir();
        let root = tmp.path();
        let dir = root.join("broken");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(PACK_SCHEMA), "{}").unwrap();
        // Missing recipe + template.
        let mut registry = ComponentRegistry::new();
        let err = registry.discover(root).expect_err("missing files");
        assert!(matches!(err, RegistryError::MissingPackFile { .. }));
    }

    #[test]
    fn validate_fields_accepts_well_formed() {
        let tmp = tempdir();
        let root = tmp.path();
        write_pack(
            root,
            "title",
            r#"{"type":"object","required":["text"],"properties":{"text":{"type":"string"}}}"#,
            "[ooxml]\nkind = \"title\"\n",
        );
        let mut registry = ComponentRegistry::new();
        registry.discover(root).unwrap();
        let id = ComponentId::new("title").unwrap();
        let merged = registry
            .validate_fields(&id, &json!({"text": "Welcome"}))
            .expect("valid");
        assert_eq!(merged["text"], "Welcome");
    }

    #[test]
    fn validate_fields_rejects_missing_required_with_pointer() {
        let tmp = tempdir();
        let root = tmp.path();
        write_pack(
            root,
            "title",
            r#"{"type":"object","required":["text"],"properties":{"text":{"type":"string"}}}"#,
            "[ooxml]\nkind = \"title\"\n",
        );
        let mut registry = ComponentRegistry::new();
        registry.discover(root).unwrap();
        let id = ComponentId::new("title").unwrap();
        let err = registry
            .validate_fields(&id, &json!({}))
            .expect_err("required missing");
        match err {
            RegistryError::SlotValidation { pointer, .. } => {
                assert_eq!(pointer, "/text");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn validate_fields_rejects_wrong_type_with_pointer() {
        let tmp = tempdir();
        let root = tmp.path();
        write_pack(
            root,
            "hero",
            r#"{"type":"object","required":["count"],"properties":{"count":{"type":"integer"}}}"#,
            "[ooxml]\nkind = \"hero\"\n",
        );
        let mut registry = ComponentRegistry::new();
        registry.discover(root).unwrap();
        let id = ComponentId::new("hero").unwrap();
        let err = registry
            .validate_fields(&id, &json!({"count": "three"}))
            .expect_err("wrong type");
        match err {
            RegistryError::SlotValidation { pointer, detail } => {
                assert_eq!(pointer, "/count");
                assert!(detail.contains("integer"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn validate_fields_merges_defaults() {
        let tmp = tempdir();
        let root = tmp.path();
        write_pack(
            root,
            "title",
            r#"{"type":"object","required":["text","color"],"properties":{"text":{"type":"string"},"color":{"type":"string"}}}"#,
            "[ooxml]\nkind = \"title\"\n",
        );
        fs::write(
            root.join("title").join(PACK_DEFAULTS),
            r#"{"color":"black"}"#,
        )
        .unwrap();
        let mut registry = ComponentRegistry::new();
        registry.discover(root).unwrap();
        let id = ComponentId::new("title").unwrap();
        let merged = registry
            .validate_fields(&id, &json!({"text": "Hello"}))
            .expect("merged");
        assert_eq!(merged["color"], "black");
        assert_eq!(merged["text"], "Hello");
    }

    #[test]
    fn validate_fields_rejects_unknown_component() {
        let registry = ComponentRegistry::new();
        let id = ComponentId::new("nope").unwrap();
        let err = registry
            .validate_fields(&id, &json!({}))
            .expect_err("unknown");
        assert!(matches!(err, RegistryError::UnknownComponent { .. }));
    }

    #[test]
    fn malformed_schema_is_surfaced_on_discovery() {
        let tmp = tempdir();
        let root = tmp.path();
        let dir = root.join("bad");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(PACK_SCHEMA), "{this is not json").unwrap();
        fs::write(dir.join(PACK_RECIPE), "[ooxml]\n").unwrap();
        fs::write(dir.join(PACK_TEMPLATE), "<div/>").unwrap();
        let mut registry = ComponentRegistry::new();
        let err = registry.discover(root).expect_err("bad schema");
        assert!(matches!(err, RegistryError::MalformedSchema { .. }));
    }

    fn tempdir() -> TestTempDir {
        TestTempDir::new()
    }

    /// Minimal scoped tempdir helper that avoids pulling the `tempfile`
    /// crate as a workspace dep. Creates a unique directory under the
    /// system tempdir and removes it on drop.
    struct TestTempDir {
        path: PathBuf,
    }

    impl TestTempDir {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let pid = std::process::id();
            let path = std::env::temp_dir().join(format!("poiesis-core-test-{pid}-{n}"));
            fs::create_dir_all(&path).expect("create tempdir");
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
}
