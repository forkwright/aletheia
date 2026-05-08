use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use koina::id::ToolName;

use super::*;
use crate::types::{InputSchema, Reversibility, ToolCategory, ToolGroupId, ToolTag};

struct MockExecutor {
    calls: Arc<Mutex<Vec<ToolName>>>,
    response: String,
}

impl ToolExecutor for MockExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a crate::types::ToolInput,
        _ctx: &'a crate::types::ToolContext,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<crate::types::ToolResult>> + Send + 'a>>
    {
        Box::pin(async {
            self.calls
                .lock()
                .expect("lock poisoned")
                .push(input.name.clone());
            Ok(crate::types::ToolResult::text(self.response.clone()))
        })
    }
}

fn mock_executor(response: &str) -> (Box<dyn ToolExecutor>, Arc<Mutex<Vec<ToolName>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let executor = Box::new(MockExecutor {
        calls: Arc::clone(&calls),
        response: response.to_owned(),
    });
    (executor, calls)
}

fn make_def_with_tags(name: &str, category: ToolCategory, tags: Vec<ToolTag>) -> ToolDef {
    ToolDef {
        name: ToolName::new(name).expect("valid test tool name"),
        description: format!("Test tool: {name}"),
        extended_description: None,
        input_schema: InputSchema {
            properties: indexmap::IndexMap::new(),
            required: vec![],
        },
        category,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read],
        tags,
    }
}

#[test]
fn test_definitions_for_tags_recon() {
    let mut reg = ToolRegistry::new();
    let (e1, _) = mock_executor("ok");
    let (e2, _) = mock_executor("ok");
    let (e3, _) = mock_executor("ok");
    reg.register(
        make_def_with_tags("read", ToolCategory::Workspace, vec![ToolTag::Recon]),
        e1,
    )
    .expect("register");
    reg.register(
        make_def_with_tags("write", ToolCategory::Workspace, vec![ToolTag::Edit]),
        e2,
    )
    .expect("register");
    reg.register(
        make_def_with_tags("grep", ToolCategory::Workspace, vec![ToolTag::Recon]),
        e3,
    )
    .expect("register");

    let recon = reg.definitions_for_tags(&[ToolTag::Recon]);
    let names: Vec<&str> = recon.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"read"), "read should be Recon-tagged");
    assert!(names.contains(&"grep"), "grep should be Recon-tagged");
    assert!(
        !names.contains(&"write"),
        "write should not be in Recon results"
    );
}

#[test]
fn test_multiple_tags_union() {
    let mut reg = ToolRegistry::new();
    let (e1, _) = mock_executor("ok");
    let (e2, _) = mock_executor("ok");
    let (e3, _) = mock_executor("ok");
    reg.register(
        make_def_with_tags("fetch", ToolCategory::Research, vec![ToolTag::Fetch]),
        e1,
    )
    .expect("register");
    reg.register(
        make_def_with_tags("scan", ToolCategory::System, vec![ToolTag::Recon]),
        e2,
    )
    .expect("register");
    reg.register(
        make_def_with_tags("exec", ToolCategory::Workspace, vec![ToolTag::Execute]),
        e3,
    )
    .expect("register");

    let union = reg.definitions_for_tags(&[ToolTag::Recon, ToolTag::Fetch]);
    let names: Vec<&str> = union.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"fetch"), "fetch matches Fetch tag");
    assert!(names.contains(&"scan"), "scan matches Recon tag");
    assert!(
        !names.contains(&"exec"),
        "exec should not match Recon or Fetch"
    );
}

#[test]
fn test_empty_tag_list_empty_result() {
    let mut reg = ToolRegistry::new();
    let (e1, _) = mock_executor("ok");
    reg.register(
        make_def_with_tags("read", ToolCategory::Workspace, vec![ToolTag::Recon]),
        e1,
    )
    .expect("register");

    let result = reg.definitions_for_tags(&[]);
    assert!(result.is_empty(), "empty tag list should return empty vec");
}

#[test]
fn test_definitions_for_category_unchanged() {
    let mut reg = ToolRegistry::new();
    let (e1, _) = mock_executor("ok");
    let (e2, _) = mock_executor("ok");
    let (e3, _) = mock_executor("ok");
    reg.register(
        make_def_with_tags("read", ToolCategory::Workspace, vec![ToolTag::Recon]),
        e1,
    )
    .expect("register");
    reg.register(
        make_def_with_tags("note", ToolCategory::Memory, vec![ToolTag::Edit]),
        e2,
    )
    .expect("register");
    reg.register(
        make_def_with_tags("grep", ToolCategory::Workspace, vec![ToolTag::Recon]),
        e3,
    )
    .expect("register");

    let ws = reg.definitions_for_category(ToolCategory::Workspace);
    assert_eq!(ws.len(), 2, "workspace category should still have 2 tools");
    let mem = reg.definitions_for_category(ToolCategory::Memory);
    assert_eq!(mem.len(), 1, "memory category should still have 1 tool");
    let comm = reg.definitions_for_category(ToolCategory::Communication);
    assert!(comm.is_empty(), "communication category should be empty");
}
