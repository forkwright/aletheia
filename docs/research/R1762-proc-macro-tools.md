# R1762: Evaluate Typst `#[func]` Proc Macro Pattern for Tool Definition

**Date:** 2026-03-19
**Author:** Research agent
**Status:** Final
**Closes:** #1762

---

## Executive Summary

Typst's `#[func]` attribute macro transforms an annotated Rust function into a fully-typed function record with name, description, parameter schemas, and a dispatch closure — all derived from doc comments and parameter annotations at compile time. This is the **exact problem** that aletheia's tool definition layer faces: 33 built-in tools each require a separate `_def()` function (averaging 40–60 lines of manual `ToolDef` construction) plus a `struct Executor; impl ToolExecutor for Executor { ... }` block. An `#[tool]` macro modelled on `#[func]` would eliminate most of this boilerplate while making schema definitions self-documenting and co-located with the implementation.

**Recommendation: Build `aletheia-macros` with an `#[tool]` macro.** The typst `func.rs` macro (475 lines) is the reference implementation. The adaptation is straightforward: replace typst's `Args` bag with `serde_json::Value` deserialization and replace the synchronous return path with `Box::pin(async move { ... })`. The break-even point (one-time macro cost vs. boilerplate saved) is reached at ~8 tools. Aletheia has 33.

---

## 1. Current Aletheia Tool Definition Pattern

Each built-in tool in `crates/organon/src/builtins/` requires three separate artifacts:

### 1.1 Schema function (`_def()`)

```rust
// crates/organon/src/builtins/filesystem.rs — grep tool def (59 lines)
fn grep_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("grep"),
        description: "Search file contents using ripgrep".into(),
        extended_description: Some("...".into()),
        input_schema: InputSchema {
            properties: [
                ("pattern".into(), PropertyDef {
                    property_type: PropertyType::String,
                    description: "Search pattern (regex supported)".into(),
                    enum_values: None,
                    default: None,
                }),
                ("path".into(), PropertyDef { ... }),
                // ... 3 more properties
            ].into_iter().collect(),
            required: vec!["pattern".into()],
        },
        category: ToolCategory::Workspace,
        auto_activate: true,
    }
}
```

### 1.2 Executor struct + trait impl

```rust
struct GrepExecutor;

impl ToolExecutor for GrepExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let pattern = input.arguments["pattern"].as_str()
                .ok_or_else(|| error::MissingArgumentSnafu { name: "pattern" }.build())?;
            // ... 5 more extractions
            // ... implementation
        })
    }
}
```

### 1.3 Registration call

```rust
// crates/organon/src/builtins/mod.rs
registry.register(grep_def(), Box::new(GrepExecutor))?;
```

Total: ~100 lines per tool, ~3,300 lines of boilerplate across 33 tools.

---

## 2. Typst `#[func]` Pattern

### 2.1 Source location

- `crates/typst-macros/src/func.rs` — main proc macro implementation (~475 lines)
- `crates/typst-macros/src/lib.rs` — exports `#[func]`, `#[elem]`, `#[ty]`
- `crates/typst-library/src/foundations/func.rs` — `NativeFuncData`, `NativeFunc` trait

### 2.2 Usage example

```rust
/// Determines the minimum of a sequence of values.
#[func(title = "Minimum", category = "math")]
fn min(
    /// The values to extract the minimum from.
    #[variadic]
    values: Vec<i64>,
    /// A default value if there are no values.
    #[named]
    #[default(0)]
    default: i64,
) -> i64 {
    values.iter().min().copied().unwrap_or(default)
}
```

### 2.3 What the macro generates

```rust
// 1. Shadow type in the type namespace
enum min {}

// 2. NativeFunc impl (returns a static data blob)
impl NativeFunc for min {
    fn data() -> &'static NativeFuncData {
        static DATA: NativeFuncData = NativeFuncData {
            function: NativeFuncPtr(&|engine, ctx, args| {
                // Generated dispatch closure:
                let values: Vec<i64> = args.all()?;
                let default: i64 = args.eat()?.unwrap_or(0);
                args.take().finish()?;
                let output = min(values, default);
                IntoResult::into_result(output, args.span)
            }),
            name: "min",
            title: "Minimum",
            docs: "Determines the minimum of a sequence of values.",
            params: DynLazyLock::new(|| vec![
                NativeParamInfo {
                    name: "values",
                    docs: "The values to extract the minimum from.",
                    variadic: true,
                    required: true,
                    // ... type info from Reflect trait
                },
                NativeParamInfo {
                    name: "default",
                    docs: "A default value if there are no values.",
                    named: true,
                    default: Some(|| Value::Int(0)),
                    // ...
                },
            ]),
            returns: DynLazyLock::new(|| CastInfo::of::<i64>()),
        };
        &DATA
    }
}
```

### 2.4 Key macro mechanisms

**Doc comment extraction** (`util::documentation(&item.attrs)`):
- Walks `syn::Attribute` list for `#[doc = "..."]` (desugared from `///`)
- Strips leading space, joins with `\n`, trims
- Applied identically to the function and to each `syn::FnArg`

**Parameter attribute handling**:

| Attribute | Effect |
|---|---|
| `#[named]` | Parses from `args` by name; type must be `Option<T>` or have `#[default]` |
| `#[default]` | Uses `Default::default()` |
| `#[default(expr)]` | Uses the given expression |
| `#[variadic]` | Consumes all remaining positional args; type must be `Vec<T>` |
| `#[external]` | Docs-only; stripped from real function signature |

**Special parameter interception** (not parsed from args):
- `engine: &mut Engine`
- `context: Tracked<Context>`
- `args: &mut Args`
- `span: Span`

**Name derivation**: function ident → kebab-case (e.g., `read_file` → `"read-file"`), unless `#[func(name = "...")]` overrides.

**Lazy parameter initialization**: `params` and `returns` use `DynLazyLock` (lazy_static equivalent) to break initialization cycles when types reference each other through `CastInfo`.

---

## 3. Proposed `#[tool]` Macro Design for Aletheia

### 3.1 Usage

```rust
use aletheia_macros::tool;

/// Search file contents for a pattern.
///
/// Uses ripgrep internally. Supports full regex syntax.
#[tool(category = "workspace", auto_activate)]
async fn grep(
    /// Search pattern (regex or literal string).
    pattern: String,
    /// File or directory to search. Defaults to workspace root.
    #[default]
    path: Option<String>,
    /// Restrict to files matching this glob.
    #[default]
    include: Option<String>,
    /// Maximum number of results to return.
    #[default(100)]
    limit: u64,
    // Special param — injected by dispatch, not parsed from JSON:
    ctx: &ToolContext,
) -> Result<ToolResult> {
    // implementation
}
```

### 3.2 Generated artifacts

The macro generates:
1. The original async fn (cleaned of macro attributes)
2. A `fn grep_tool_registration() -> (ToolDef, Box<dyn ToolExecutor>)` free function

```rust
// Generated registration factory:
pub fn grep_tool_registration() -> (ToolDef, Box<dyn ToolExecutor>) {
    let def = ToolDef {
        name: ToolName::from_static("grep"),
        description: "Search file contents for a pattern.".into(),
        extended_description: Some("Uses ripgrep internally. Supports full regex syntax.".into()),
        input_schema: InputSchema {
            properties: [
                ("pattern", PropertyDef { property_type: PropertyType::String,
                    description: "Search pattern (regex or literal string).".into(),
                    ..Default::default() }),
                ("path", PropertyDef { property_type: PropertyType::String,
                    description: "File or directory to search. Defaults to workspace root.".into(),
                    ..Default::default() }),
                // ... limit, include
            ].into_iter().collect(),
            required: vec!["pattern".into()],
        },
        category: ToolCategory::Workspace,
        auto_activate: true,
    };

    struct GrepExecutor;
    impl ToolExecutor for GrepExecutor {
        fn execute<'a>(&'a self, input: &'a ToolInput, ctx: &'a ToolContext)
            -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>>
        {
            Box::pin(async move {
                let pattern: String = serde_json::from_value(
                    input.arguments["pattern"].clone()
                ).map_err(|_| error::MissingArgumentSnafu { name: "pattern" }.build())?;
                let path: Option<String> = input.arguments.get("path")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());
                // ... other params
                grep(pattern, path, include, limit, ctx).await
            })
        }
    }

    (def, Box::new(GrepExecutor))
}
```

### 3.3 `register_all()` change

```rust
// Before (manual):
registry.register(grep_def(), Box::new(GrepExecutor))?;

// After (generated):
let (def, exec) = grep_tool_registration();
registry.register(def, exec)?;
```

Or with a helper macro:
```rust
register_tools!(registry, [grep, read_file, write_file, ...]);
```

### 3.4 JSON schema type mapping

| Rust type | `PropertyType` |
|---|---|
| `String` / `&str` | `String` |
| `u64` / `i64` / `usize` | `Integer` |
| `f64` / `f32` | `Number` |
| `bool` | `Boolean` |
| `Vec<T>` | `Array` |
| `Option<T>` | same as `T` (not required) |
| custom type with `#[schema(object)]` | `Object` |

Enum variants map to `enum_values: Some(vec![...])` when annotated with `#[schema(enum = ["a", "b"])]`.

---

## 4. Implementation Plan

### 4.1 New crate: `aletheia-macros`

```
crates/
  aletheia-macros/
    Cargo.toml          # proc-macro = true, deps: syn, quote, proc-macro2
    src/
      lib.rs            # #[proc_macro_attribute] pub fn tool(...)
      tool.rs           # main impl (~400 lines, modelled on typst func.rs)
      util.rs           # documentation() extractor, name_to_kebab_case()
```

Dependencies: `syn = { features = ["full"] }`, `quote`, `proc-macro2`. No runtime dependencies.

### 4.2 Migration path

Migration is zero-risk: the macro and the manual pattern are interchangeable at the registration call. Migrate tools incrementally by file:
1. Start with `filesystem.rs` (largest — 5 tools, most boilerplate)
2. Continue with `memory.rs`, `workspace.rs`
3. Leave any tools with unusually complex dispatch (e.g., sandbox-aware conditionals) for last

### 4.3 Effort estimate

| Phase | Effort |
|---|---|
| `aletheia-macros` crate + `#[tool]` macro | ~2–3 days |
| Migrate all 33 tools | ~1–2 days |
| Add `register_tools!` helper | ~0.5 days |

---

## 5. Evidence

- **Typst `func.rs`**: 475-line reference implementation at `crates/typst-macros/src/func.rs` — handles doc extraction, parameter annotation parsing, name derivation, dispatch closure generation
- **`util::documentation()`**: Doc comment extraction from `syn::Attribute` — 15 lines, directly reusable
- **Boilerplate baseline**: `grep_def()` in `crates/organon/src/builtins/filesystem.rs:374–433` (59 lines) + `GrepExecutor` impl (~30 lines) vs. expected ~15-line `#[tool]` annotated function
- **33 tools × ~90 lines average** = ~3,000 lines of boilerplate targeted

---

## 6. Recommendation

Build `aletheia-macros` with `#[tool]`. The typst `func.rs` solves the same problem (Rust function → typed registry entry with doc-derived schema) and is the direct template. The primary adaptation is:

1. `Args` bag → `serde_json::Value` deserialization
2. Synchronous `IntoResult` → `Box::pin(async move { ... })` wrapper
3. `NativeFuncData` static → `(ToolDef, Box<dyn ToolExecutor>)` factory function

The macro reduces every new tool from ~100 lines to ~20 lines, makes parameter schemas self-documenting, and eliminates the class of bugs where `_def()` and the actual extraction code disagree on parameter names or types. It also becomes the natural integration point for the `ValidationAccumulator` from R1763.
