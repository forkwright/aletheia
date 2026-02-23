# Credential-Based Repository Investigation and Provider Analysis

Investigate a codebase's provider implementations and authentication system by retrieving credentials and analyzing git history and provider-specific code files.

## When to Use
When you need to understand how a project authenticates with external services (e.g., API providers), understand recent changes to authentication or setup workflows, and examine provider-specific implementation details.

## Steps
1. Retrieve stored credentials from the local credentials directory
2. Navigate to the project repository and examine recent git commits to identify relevant changes
3. Check git diff against the previous commit to see which files were modified
4. Read the provider router/abstraction layer to understand the provider architecture
5. Read provider-specific implementation files (e.g., the particular provider being investigated)
6. Verify current repository status and stash state to understand working directory context
7. Validate credential access one more time to confirm authentication setup

## Tools Used
- exec: Navigate to repository, view git history, check git diffs, verify repository status and stash state
- read: Access provider router and provider-specific implementation files to understand authentication integration