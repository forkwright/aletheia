#!/usr/bin/env bash
# Install the in-tree git hooks (one-time per clone; idempotent).
#
# Points git at scripts/githooks/ (version-controlled, so every clone gets the
# same fmt/clippy/_llm pre-push + instance-guard pre-commit). Auto-run by .envrc
# on direnv load; run manually if you don't use direnv.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
git config core.hooksPath scripts/githooks
chmod +x scripts/githooks/* 2>/dev/null || true
echo "git hooks installed: core.hooksPath=scripts/githooks (pre-push, pre-commit)"
