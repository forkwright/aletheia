# Issue Triage and Analysis
Systematically retrieve and review open GitHub issues to understand project backlog and prioritize work items.

## When to Use
When you need to understand the current state of a project's issue backlog, identify high-priority items, or gather context about specific problems that need attention.

## Steps
1. List all open issues with basic metadata (number, title, creation date) to get an overview
2. Retrieve issues with structured data (number, title, labels) and sort by issue number to see categorized work
3. Sequentially fetch the detailed body/context of specific issues of interest to understand requirements and acceptance criteria
4. Analyze the retrieved issue contexts to identify patterns, dependencies, and priorities

## Tools Used
- exec: Execute GitHub CLI commands to query issues at different levels of detail (list, view with JSON output)
- gh issue list: Retrieve open issues with filtering and formatting options
- gh issue view: Fetch full issue details including body content and metadata