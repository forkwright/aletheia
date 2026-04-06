//! Property-based tests for the tool registry.
//!
//! Enabled by `--features test-support`.
//! Failing seeds are stored in `tests/proptest-regressions/proptest_registry.txt`.
//!
//! Run: `cargo test -p aletheia-organon --features test-support --test proptest_registry`

#![expect(
    clippy::unwrap_used,
    reason = "proptest test bodies may panic on failed assertions"
)]
#![expect(
    clippy::expect_used,
    reason = "proptest: ToolName::new() with controlled inputs; expect message documents constraint"
)]

use aletheia_koina::id::ToolName;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::testing::{MockToolExecutor, make_test_context, make_tool_input};
use aletheia_organon::types::{InputSchema, Reversibility, ToolCategory, ToolDef};
use indexmap::IndexMap;
use proptest::prelude::*;

fn valid_tool_name() -> impl Strategy<Value = String> {
    // Tool names: lowercase alphanumeric + underscore, 1-32 chars
    "[a-z][a-z0-9_]{0,31}".prop_map(|s| s)
}

fn make_def(name: &str) -> ToolDef {
    ToolDef {
        name: ToolName::new(name).expect("valid tool name from strategy"),
        description: format!("mock tool: {name}"),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::new(),
            required: vec![],
        },
        category: ToolCategory::System,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
    }
}

proptest! {
    /// Registering N distinct tools makes them all discoverable by name.
    #[test]
    fn registry_finds_all_registered_tools(
        names in proptest::collection::hash_set(valid_tool_name(), 1..=8),
    ) {
        let mut registry = ToolRegistry::new();
        let names: Vec<String> = names.into_iter().collect();
        for name in &names {
            let def = make_def(name);
            let tool_name = def.name.clone();
            registry.register(def, Box::new(MockToolExecutor::text("ok").named(tool_name))).unwrap();
        }
        for name in &names {
            let tool_name = ToolName::new(name).expect("valid tool name");
            prop_assert!(
                registry.get_def(&tool_name).is_some(),
                "registered tool '{name}' must be retrievable"
            );
        }
    }

    /// Registering the same name twice always fails.
    #[test]
    fn registry_rejects_duplicate_names(name in valid_tool_name()) {
        let mut registry = ToolRegistry::new();
        let def1 = make_def(&name);
        let tn1 = def1.name.clone();
        registry.register(def1, Box::new(MockToolExecutor::text("first").named(tn1))).unwrap();

        let def2 = make_def(&name);
        let tn2 = def2.name.clone();
        let second = registry.register(def2, Box::new(MockToolExecutor::text("second").named(tn2)));
        prop_assert!(
            second.is_err(),
            "registering duplicate name '{name}' must return Err"
        );
    }

    /// Executing a registered tool returns Ok, not a ToolNotFound error.
    #[test]
    fn registry_executes_registered_tool(name in valid_tool_name()) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");

        rt.block_on(async {
            let mut registry = ToolRegistry::new();
            let def = make_def(&name);
            let tn = def.name.clone();
            registry.register(def, Box::new(MockToolExecutor::text("result").named(tn.clone()))).unwrap();

            let input = make_tool_input(&tn);
            let ctx = make_test_context();
            let result = registry.execute(&input, &ctx).await;
            prop_assert!(result.is_ok(), "executing registered tool '{name}' must succeed");
            Ok(())
        }).unwrap();
    }
}
