# GitHub Issue Investigation and Analysis
Systematically retrieve and analyze multiple GitHub issues to understand project problems, bugs, and feature requests.

## When to Use
When you need to investigate a repository's issue backlog to understand recurring problems, prioritize bugs, gather context about multiple related issues, or extract information from issue discussions for analysis or reporting.

## Steps
1. List all open issues with key metadata (number, title, labels, state) to get an overview
2. Identify specific issue numbers of interest from the list
3. Retrieve full body content of selected issues using issue view commands
4. Extract and examine the first ~20 lines of issue descriptions to understand context
5. Batch retrieve multiple related issues in a loop to efficiently gather information on several topics
6. Parse JSON output with jq to extract structured data (body text, labels, etc.)

## Tools Used
- exec: Execute shell commands to run GitHub CLI (gh) commands for querying issues
- gh issue list: Retrieve multiple issues with filtered metadata
- gh issue view: Get detailed information for specific issue numbers
- jq: Parse and filter JSON output to extract relevant fields
