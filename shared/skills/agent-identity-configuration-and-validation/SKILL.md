# Agent Identity Configuration and Validation
Initialize and validate agent identity profiles across a distributed system, extracting and standardizing identity metadata.

## When to Use
When setting up or updating identity profiles for multiple agents in a system, need to verify configuration consistency, or extract identity metadata (like emojis or names) across agent files.

## Steps
1. Edit the target identity file to remove template boilerplate and set core identity fields
2. Create or overwrite identity configuration files with structured identity data
3. Query the configuration system to verify identity settings across all agents
4. Search for identity-related schema definitions and field requirements in codebase
5. Validate parsing of identity metadata (like emoji extraction) using pattern matching
6. Cross-reference identity configurations across multiple agent files to ensure consistency

## Tools Used
- edit: modify identity template files to add agent-specific information
- exec: execute shell commands to write configuration files and test metadata parsing
- grep: search codebase for identity-related schema definitions and field requirements
