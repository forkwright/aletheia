from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "generate-configuration-doc.py"
SPEC = importlib.util.spec_from_file_location("generate_configuration_doc", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {SCRIPT_PATH}")
doc_mod = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = doc_mod
SPEC.loader.exec_module(doc_mod)


class SchemaFixtureTestCase(unittest.TestCase):
    """WHY: parse_type()/read_file()/locate_type_file() memoize by name and
    path in module-level dicts so a real run never re-walks the repo tree
    twice for the same type. Each test writes its own synthetic schema
    tree under a fresh CONFIG_DIR/REPO_ROOT, so those caches must be
    cleared per test -- otherwise a struct/enum name reused by an earlier
    test's fixture would silently answer from the wrong file, or a lookup
    miss would fall through to the real multi-thousand-file aletheia tree
    this repo is running inside.
    """

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.repo_root = Path(tmp.name)
        self.config_dir = self.repo_root / "crates" / "taxis" / "src" / "config"
        self.config_dir.mkdir(parents=True)

        for name, value in (("REPO_ROOT", self.repo_root), ("CONFIG_DIR", self.config_dir)):
            patcher = mock.patch.object(doc_mod, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

        for cache_name in (
            "_FILE_CACHE",
            "_TYPE_FILE_CACHE",
            "_TYPE_FILE_LOCAL_CACHE",
            "_TYPE_ALIAS_CACHE",
            "_TYPE_CACHE",
            "_USE_IMPORTS_CACHE",
            "_LOCALS_CACHE",
        ):
            getattr(doc_mod, cache_name).clear()
        doc_mod._CONFIG_FILES = None
        doc_mod._REPO_TYPE_INDEX = None
        doc_mod._REPO_ALIAS_INDEX = None

    def write_rs(self, name: str, content: str) -> Path:
        path = self.config_dir / name
        path.write_text(content, encoding="utf-8")
        return path


# ── bug 1: scalar_default() must read Option<T> as None, not T's default ───


class OptionDefaultRegressionTests(SchemaFixtureTestCase):
    def setUp(self) -> None:
        super().setUp()
        self.write_rs(
            "provider.rs",
            '''\
use serde::{Deserialize, Serialize};

/// One configured LLM provider.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct LlmProviderConfig {
    /// Operator-facing label.
    pub name: String,
    /// HTTP base URL override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Wall-clock timeout for subprocess provider calls, in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// `OpenAI` API family to use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_family: Option<OpenAiApiFamily>,
}

/// Which `OpenAI`-style wire format a provider speaks.
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OpenAiApiFamily {
    #[default]
    Responses,
    ChatCompletions,
}
''',
        )
        parsed = doc_mod.parse_type("LlmProviderConfig")
        assert isinstance(parsed, doc_mod.StructDef)
        self.container = parsed
        self.fields_by_name = {fd.name: fd for fd in parsed.fields}

    def test_option_string_renders_unset_not_empty_string(self) -> None:
        # WHY: the old scalar_default() computed structural_default(T) --
        # here T=String, giving `""` -- before ever checking that the field
        # was Option<T>, whose real serde-default is always None.
        fd = self.fields_by_name["base_url"]
        self.assertEqual(doc_mod.scalar_default(fd, self.container), "unset")

    def test_option_u64_renders_unset_not_zero(self) -> None:
        fd = self.fields_by_name["timeout_secs"]
        self.assertEqual(doc_mod.scalar_default(fd, self.container), "unset")

    def test_option_enum_renders_unset_not_first_variant(self) -> None:
        # WHY: OpenAiApiFamily's first variant carries #[default] and has_serde
        # is True, so the pre-fix code path would render `"responses"` --
        # a real value the field is never actually assigned when omitted.
        fd = self.fields_by_name["api_family"]
        self.assertEqual(doc_mod.scalar_default(fd, self.container), "unset")


# ── bug 2: enum_default_variant() must not fabricate a first-variant default ─


class EnumDefaultFallbackRegressionTests(SchemaFixtureTestCase):
    def setUp(self) -> None:
        super().setUp()
        self.write_rs(
            "deployment.rs",
            '''\
/// Where a provider's traffic terminates. No variant is #[default] and
/// there is no #[derive(Default)] -- this generator cannot see whether a
/// hand-written `impl Default` exists elsewhere.
pub enum DeploymentTarget {
    Local,
    Cloud,
    Hybrid,
}

/// A container that names DeploymentTarget::default() explicitly.
#[derive(Clone)]
pub struct RoutingConfig {
    pub deployment_target: DeploymentTarget,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self { deployment_target: DeploymentTarget::default() }
    }
}
''',
        )

    def test_no_default_variant_returns_none_not_first_declared(self) -> None:
        enum_def = doc_mod.parse_type("DeploymentTarget")
        assert isinstance(enum_def, doc_mod.EnumDef)
        # WHY: the pre-fix fallback returned enum_def.variants[0] (Local)
        # whenever no variant carried #[default]. Local is not a proven
        # default -- it is just the first line in the source file.
        self.assertIsNone(doc_mod.enum_default_variant(enum_def))

    def test_field_default_renders_the_expression_not_a_fabricated_variant(self) -> None:
        struct_def = doc_mod.parse_type("RoutingConfig")
        assert isinstance(struct_def, doc_mod.StructDef)
        fd = struct_def.fields[0]
        rendered = doc_mod.scalar_default(fd, struct_def)
        self.assertEqual(rendered, "`DeploymentTarget::default()`")
        self.assertNotIn("Local", rendered)


# ── bug 3: non_unit_enum_for() must gate tagged-enum classification on serde ─


class NonUnitEnumTaggingRegressionTests(SchemaFixtureTestCase):
    def setUp(self) -> None:
        super().setUp()
        self.write_rs(
            "tool_policy.rs",
            '''\
/// Tool-group policy for an agent. Hand-written Serialize/Deserialize
/// accept a bare string ("all"/"deny") or a bare array -- not tagged on
/// any field at all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AgentToolGroupPolicy {
    /// All tool groups are permitted.
    AllowAll,
    /// Only tools with one of these groups are permitted.
    Groups(Vec<String>),
    /// No tool groups are permitted.
    #[default]
    DenyAll,
}
''',
        )
        self.write_rs(
            "retry.rs",
            '''\
use serde::{Deserialize, Serialize};

/// Retry policy, genuinely tagged on `type`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RetryPolicy {
    /// Fixed-delay retries.
    Fixed {
        /// Number of attempts.
        attempts: u32,
    },
    /// Exponential backoff.
    Exponential {
        /// Base delay in milliseconds.
        base_ms: u64,
    },
}
''',
        )

    def test_hand_written_deserialize_enum_is_not_classified_as_tagged(self) -> None:
        # WHY: the pre-fix classifier keyed on "has a non-unit variant"
        # alone, so AgentToolGroupPolicy (Groups(Vec<String>) is non-unit)
        # was misclassified as #[serde(tag = "type")]-shaped and rendered
        # hardcoded "Tagged on `type`" prose it never actually uses.
        self.assertIsNone(doc_mod.non_unit_enum_for("AgentToolGroupPolicy"))

    def test_untagged_enum_type_label_is_the_bare_type_not_a_variant_list(self) -> None:
        label = doc_mod.simplify_scalar_type("AgentToolGroupPolicy")
        self.assertEqual(label, "`AgentToolGroupPolicy`")

    def test_genuinely_serde_tagged_enum_is_still_classified(self) -> None:
        # WHY: a bare has_serde gate that always returned None would be as
        # wrong as no gate at all. RetryPolicy actually derives Deserialize
        # and has non-unit variants, so it must still be recognized.
        result = doc_mod.non_unit_enum_for("RetryPolicy")
        self.assertIsNotNone(result)
        self.assertEqual(result.name, "RetryPolicy")


# ── bug 4: `pub type X = path::Y;` aliases must resolve to Y, not to a ─────
# ── same-named struct/enum found elsewhere in the repo ─────────────────────


class CrossCrateTypeAliasRegressionTests(SchemaFixtureTestCase):
    """WHY (aletheia#6741 fallout): consolidating two duplicate pricing
    types left `pub type ModelPricing = koina::models::ModelPrice;` in the
    config module. `locate_type_file` only ever looks for `pub struct`/
    `pub enum` declarations, never `pub type`, so resolving the bare name
    `ModelPricing` fell straight through to the repo-wide struct/enum name
    index -- which happened to also match an unrelated `ModelPricing` DTO
    struct in a REST-handler crate (same name, same field names, zero doc
    comments). The generator rendered that decoy's undocumented fields
    instead of following the alias to the real, documented type. This
    fixture reproduces that exact shape: a same-named decoy struct in one
    crate, and the real aliased target (with field docs) in another.
    """

    def setUp(self) -> None:
        super().setUp()
        self.write_rs(
            "mod.rs",
            '''\
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Root configuration for an Aletheia instance.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AletheiaConfig {
    /// Per-model pricing for LLM cost metrics. Keyed by model name.
    pub pricing: HashMap<String, ModelPricing>,
}

/// Per-model pricing rates for cost estimation in metrics.
pub type ModelPricing = otherkrate::models::ModelPrice;
''',
        )
        # decoy: an unrelated, undocumented struct of the SAME name in a
        # different crate -- what the repo-wide name index finds first if
        # the alias above is never followed.
        decoy_dir = self.repo_root / "crates" / "decoykrate" / "src"
        decoy_dir.mkdir(parents=True)
        (decoy_dir / "handlers.rs").write_text(
            '''\
use serde::Deserialize;

/// Schema for a single pricing entry.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricing {
    pub input_cost_per_mtok: Option<f64>,
    pub output_cost_per_mtok: Option<f64>,
}
''',
            encoding="utf-8",
        )
        # the real aliased target, in yet another crate -- what the alias
        # in mod.rs actually points at.
        target_dir = self.repo_root / "crates" / "otherkrate" / "src"
        target_dir.mkdir(parents=True)
        (target_dir / "models.rs").write_text(
            '''\
use serde::{Deserialize, Serialize};

/// Per-model pricing rates for cost estimation.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPrice {
    /// Cost per million input tokens in USD.
    pub input_cost_per_mtok: f64,
    /// Cost per million output tokens in USD.
    pub output_cost_per_mtok: f64,
}
''',
            encoding="utf-8",
        )

    def test_alias_resolves_to_the_aliased_crate_not_the_same_named_decoy(self) -> None:
        parsed = doc_mod.parse_type("ModelPricing")
        assert isinstance(parsed, doc_mod.StructDef)
        self.assertEqual(parsed.name, "ModelPrice")
        self.assertEqual(parsed.source_file.name, "models.rs")
        self.assertEqual(parsed.source_file.parent.parent.name, "otherkrate")

    def test_field_docs_come_from_the_aliased_type_not_the_decoy(self) -> None:
        parsed = doc_mod.parse_type("ModelPricing")
        assert isinstance(parsed, doc_mod.StructDef)
        docs = {fd.name: fd.doc for fd in parsed.fields}
        self.assertEqual(docs["input_cost_per_mtok"], "Cost per million input tokens in USD.")
        self.assertEqual(docs["output_cost_per_mtok"], "Cost per million output tokens in USD.")

    def test_rendered_reference_carries_the_aliased_field_docs_not_blank_cells(self) -> None:
        rendered = doc_mod.build_reference()
        self.assertIn(
            "| `inputCostPerMtok` | float | *required* | Cost per million input tokens in USD. |",
            rendered,
        )
        self.assertIn(
            "| `outputCostPerMtok` | float | *required* | Cost per million output tokens in USD. |",
            rendered,
        )
        # WHY a separate negative assertion: an empty Description cell and a
        # populated one both satisfy "there is a row for this field" -- the
        # bug this pins is specifically a blank cell, which the positive
        # assertions above cannot rule out on their own (a decoy row with
        # the same field names and type would also match a loose "contains
        # inputCostPerMtok" check).
        self.assertNotIn("| `inputCostPerMtok` | float | *required* |  |", rendered)
        self.assertNotIn("| `outputCostPerMtok` | float | *required* |  |", rendered)


# ── fail-closed parsing, generically ────────────────────────────────────────


class FailClosedResolutionTests(SchemaFixtureTestCase):
    def test_unresolvable_call_expression_renders_as_backticked_source(self) -> None:
        # WHY: this module's own docstring promises an unresolvable default
        # is "rendered as the literal Rust expression rather than guessed."
        # A path-qualified call to a helper this generator has no way to
        # evaluate must come back as that Rust source, not an invented "0"
        # or empty value.
        rendered = doc_mod.resolve_value(
            "some_crate::compute_default_timeout()",
            local_consts={},
            locals_map={},
            depth=0,
            source_file=None,
        )
        self.assertEqual(rendered, "`some_crate::compute_default_timeout()`")

    def test_depth_exhaustion_renders_source_rather_than_guessing(self) -> None:
        rendered = doc_mod.resolve_value(
            "whatever_this_is",
            local_consts={},
            locals_map={},
            depth=doc_mod.MAX_RESOLVE_DEPTH + 1,
            source_file=None,
        )
        self.assertEqual(rendered, "`whatever_this_is`")


# ── build_reference(): determinism + no wall-clock leakage ─────────────────


MINIMAL_SCHEMA = '''\
use serde::{Deserialize, Serialize};

/// Root configuration for an Aletheia instance.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AletheiaConfig {
    /// Retry attempts before giving up.
    pub retries: u32,
    /// HTTP gateway settings.
    pub gateway: GatewayConfig,
    /// Configured LLM providers.
    pub providers: Vec<ProviderEntry>,
}

impl Default for AletheiaConfig {
    fn default() -> Self {
        Self { retries: 3, gateway: GatewayConfig::default(), providers: Vec::new() }
    }
}

/// HTTP gateway settings.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct GatewayConfig {
    /// TCP port the gateway listens on.
    pub port: u16,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self { port: 8080 }
    }
}

/// One configured LLM provider entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEntry {
    /// Operator-facing label.
    pub name: String,
    /// HTTP base URL override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}
'''


class BuildReferenceDeterminismTests(SchemaFixtureTestCase):
    def setUp(self) -> None:
        super().setUp()
        self.write_rs("mod.rs", MINIMAL_SCHEMA)

    def test_render_is_byte_identical_across_calls(self) -> None:
        self.assertEqual(doc_mod.build_reference(), doc_mod.build_reference())

    def test_build_reference_has_no_embedded_generation_timestamp(self) -> None:
        # WHY: this module never imports time/datetime -- there is no
        # wall-clock read to leak into --check's compared output today.
        # Mirroring the sibling kanon generator's guard (which does read a
        # clock) pins that this stays true structurally: if a future change
        # adds a clock read anywhere in this call graph, patching the real
        # time.time here is what would catch the resulting flap.
        rendered_a = doc_mod.build_reference()
        with mock.patch("time.time", return_value=0.0):
            rendered_b = doc_mod.build_reference()
        self.assertEqual(rendered_a, rendered_b)


# ── --check: both directions ────────────────────────────────────────────────


class CheckModeTests(unittest.TestCase):
    def _write_doc(self, tmp: str, body: str) -> Path:
        doc_path = Path(tmp) / "CONFIGURATION.md"
        doc_path.write_text(
            f"# Configuration reference\n\n{doc_mod.BEGIN_MARKER}\n\n{body}\n{doc_mod.END_MARKER}\n",
            encoding="utf-8",
        )
        return doc_path

    def test_check_passes_when_doc_matches_generated_output(self) -> None:
        generated = "## Table of contents\n\n- [gateway](#gateway)\n"
        with tempfile.TemporaryDirectory() as tmp:
            doc_path = self._write_doc(tmp, generated)
            with mock.patch.object(doc_mod, "DOC_PATH", doc_path):
                with mock.patch.object(doc_mod, "build_reference", return_value=generated):
                    with mock.patch.object(
                        sys, "argv", ["generate-configuration-doc.py", "--check"]
                    ):
                        self.assertEqual(doc_mod.main(), 0)

    def test_check_fails_on_drift(self) -> None:
        # WHY: a --check that cannot fail is worse than no check at all --
        # this proves the comparison is load-bearing, not always-true.
        generated = "## Table of contents\n\n- [gateway](#gateway)\n"
        stale = "## Table of contents\n\n- [STALE](#stale)\n"
        self.assertNotEqual(generated, stale)
        with tempfile.TemporaryDirectory() as tmp:
            doc_path = self._write_doc(tmp, stale)
            with mock.patch.object(doc_mod, "DOC_PATH", doc_path):
                with mock.patch.object(doc_mod, "build_reference", return_value=generated):
                    with mock.patch.object(
                        sys, "argv", ["generate-configuration-doc.py", "--check"]
                    ):
                        self.assertEqual(doc_mod.main(), 1)

    def test_check_fails_when_doc_missing_entirely(self) -> None:
        # WHY: read_doc() used to be called unguarded here, so a deleted
        # docs/CONFIGURATION.md crashed --check with an uncaught
        # FileNotFoundError instead of the same clean ERROR-and-exit-1 the
        # missing-markers case already gets (fixed alongside this suite).
        generated = "## Table of contents\n\n- [gateway](#gateway)\n"
        with tempfile.TemporaryDirectory() as tmp:
            doc_path = Path(tmp) / "does-not-exist.md"
            with mock.patch.object(doc_mod, "DOC_PATH", doc_path):
                with mock.patch.object(doc_mod, "build_reference", return_value=generated):
                    with mock.patch.object(
                        sys, "argv", ["generate-configuration-doc.py", "--check"]
                    ):
                        self.assertEqual(doc_mod.main(), 1)


# ── anchor contract: missing markers fail loudly, not silently ─────────────


class AnchorContractTests(unittest.TestCase):
    def test_replace_generated_block_raises_on_missing_markers(self) -> None:
        with self.assertRaises(RuntimeError) as ctx:
            doc_mod.replace_generated_block("# no anchors in this doc\n", "generated content")
        self.assertIn("markers", str(ctx.exception))

    def test_check_mode_reports_missing_markers_rather_than_producing_nothing(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            doc_path = Path(tmp) / "CONFIGURATION.md"
            doc_path.write_text("# no anchors in this doc\n", encoding="utf-8")
            with mock.patch.object(doc_mod, "DOC_PATH", doc_path):
                with mock.patch.object(doc_mod, "build_reference", return_value="whatever"):
                    with mock.patch.object(
                        sys, "argv", ["generate-configuration-doc.py", "--check"]
                    ):
                        self.assertEqual(doc_mod.main(), 1)


if __name__ == "__main__":
    unittest.main()
