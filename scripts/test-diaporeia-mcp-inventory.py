#!/usr/bin/env python3
"""Fixture-based tests for scripts/generate-diaporeia-mcp-inventory.py.

Verifies that the generator parses #[tool] methods, extracts tier/role,
and classifies mutation capability deterministically from the implementation
surface.

Run with:
    uv run scripts/test-diaporeia-mcp-inventory.py
"""
from __future__ import annotations

import importlib.util
import sys
import tempfile
from pathlib import Path

_SCRIPT_PATH = Path(__file__).parent / "generate-diaporeia-mcp-inventory.py"


def _load_generator() -> object:
    spec = importlib.util.spec_from_file_location("inventory_generator", _SCRIPT_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load {_SCRIPT_PATH}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["inventory_generator"] = module
    spec.loader.exec_module(module)
    return module


GENERATOR = _load_generator()

_FAILURES: list[str] = []


def expect(condition: bool, msg: str) -> None:
    if not condition:
        _FAILURES.append(msg)


FIXTURE_SOURCE = '''\
// kanon:ignore RUST/file-too-long
//! Fixture MCP tools surface.

use rmcp::service::RequestContext;
use rmcp::{tool, tool_router};
use symbolon::types::Role;

use crate::rate_limit::Tier;
use crate::server::DiaporeiaServer;

#[tool_router(vis = "pub(crate)")]
impl DiaporeiaServer {
    /// Create a session.
    #[tool(description = "Create a session")]
    async fn session_create(
        &self,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<(), rmcp::ErrorData> {
        self.rate_limiter.check(Tier::Expensive)?;
        require_role(self, &context, Role::Operator, "session_create").await?;
        let store = self.state.session_store.lock().await;
        store.find_or_create_session("id", "nous", "main", None, None)?;
        Ok(())
    }

    /// List sessions.
    #[tool(description = "List sessions")]
    async fn session_list(
        &self,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<(), rmcp::ErrorData> {
        self.rate_limiter.check(Tier::Cheap)?;
        require_role(self, &context, Role::Agent, "session_list").await?;
        let store = self.state.session_store.lock().await;
        let _ = store.list_sessions(None)?;
        Ok(())
    }

    /// Get config.
    #[tool(description = "Get config")]
    async fn config_get(
        &self,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<(), rmcp::ErrorData> {
        self.rate_limiter.check(Tier::Cheap)?;
        require_role(self, &context, Role::Operator, "config_get").await?;
        let _ = self.state.config.read().await;
        Ok(())
    }
}
'''


def test_parses_tool_name_tier_role() -> None:
    tools = GENERATOR.parse_tools(FIXTURE_SOURCE)
    by_name = {t["name"]: t for t in tools}

    expect(
        set(by_name.keys()) == {"session_create", "session_list", "config_get"},
        f"expected three fixture tools, got {sorted(by_name.keys())}",
    )

    expect(
        by_name.get("session_create", {}).get("tier") == "Expensive",
        "session_create tier should be Expensive",
    )
    expect(
        by_name.get("session_create", {}).get("role") == "Operator",
        "session_create role should be Operator",
    )
    expect(
        by_name.get("session_list", {}).get("tier") == "Cheap",
        "session_list tier should be Cheap",
    )
    expect(
        by_name.get("session_list", {}).get("role") == "Agent",
        "session_list role should be Agent",
    )
    expect(
        by_name.get("session_create", {}).get("purpose") == "Create a session",
        "session_create purpose should come from the tool description",
    )


def test_mutation_heuristic() -> None:
    tools = GENERATOR.parse_tools(FIXTURE_SOURCE)
    by_name = {t["name"]: t for t in tools}

    expect(
        by_name.get("session_create", {}).get("mutates") == "Yes",
        "session_create mutates because it calls find_or_create_session",
    )
    expect(
        by_name.get("session_list", {}).get("mutates") == "No",
        "session_list is read-only",
    )
    expect(
        by_name.get("config_get", {}).get("mutates") == "No",
        "config_get is read-only",
    )


def test_determinism() -> None:
    a = GENERATOR.generate_block(FIXTURE_SOURCE)
    b = GENERATOR.generate_block(FIXTURE_SOURCE)
    expect(a == b, "generate_block must be deterministic")


def test_check_mode(tmp: Path) -> None:
    claude = tmp / "CLAUDE.md"
    generated = GENERATOR.generate_block(FIXTURE_SOURCE)
    claude.write_text(
        "# Fixture\n\n" + generated + "\n\n# Footer\n", encoding="utf-8"
    )

    original_path = GENERATOR.CLAUDE_PATH
    original_tools_path = GENERATOR.TOOLS_PATH
    try:
        GENERATOR.CLAUDE_PATH = claude
        GENERATOR.TOOLS_PATH = tmp / "tools.rs"
        GENERATOR.TOOLS_PATH.write_text(FIXTURE_SOURCE, encoding="utf-8")

        old_argv = sys.argv
        sys.argv = [str(_SCRIPT_PATH), "--check"]
        try:
            expect(GENERATOR.main() == 0, "check mode should pass on matching docs")

            content = claude.read_text(encoding="utf-8")
            claude.write_text(content.replace("(3)", "(99)"), encoding="utf-8")
            expect(GENERATOR.main() == 1, "check mode should fail on stale docs")
        finally:
            sys.argv = old_argv
    finally:
        GENERATOR.CLAUDE_PATH = original_path
        GENERATOR.TOOLS_PATH = original_tools_path


def main() -> int:
    test_parses_tool_name_tier_role()
    test_mutation_heuristic()
    test_determinism()

    with tempfile.TemporaryDirectory() as tmp_str:
        test_check_mode(Path(tmp_str))

    if _FAILURES:
        print(f"FAIL: {len(_FAILURES)} assertion(s) failed", file=sys.stderr)
        for failure in _FAILURES:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print("OK: all MCP inventory generator tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
