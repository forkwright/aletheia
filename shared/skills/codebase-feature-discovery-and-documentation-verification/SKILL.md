# Codebase Feature Discovery and Documentation Verification

Locate and cross-reference feature implementation across a codebase by searching for related keywords and verifying against existing documentation.

## When to Use
When you need to understand how a specific feature is implemented across a codebase, verify feature status against documentation, or trace related code patterns (e.g., finding where a feature is defined, how it's used, and what documentation exists).

## Steps
1. Search for files related to the feature using multiple keyword variations and naming conventions (e.g., both lowercase and uppercase)
2. List available documentation files to understand project structure and documentation patterns
3. Read the specification or documentation file to understand the feature's stated status and design
4. Use grep to search for key implementation details (e.g., data structures, state management) related to the feature
5. Search for related event handlers or message types that trigger the feature behavior
6. Search for visual/presentation aspects related to the feature (e.g., theming, UI components)
7. Search for infrastructure code related to the feature (e.g., connection handling, error recovery mechanisms)

## Tools Used
- exec: to run find commands for locating files and grep commands for searching code patterns
- read: to examine specification and documentation files
