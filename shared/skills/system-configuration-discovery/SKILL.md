# System Configuration Discovery
Quickly identify and extract key configuration details from a project's runtime context and version history.

## When to Use
When you need to understand the current setup, model configuration, or recent changes to a system without manually parsing multiple files.

## Steps
1. List the directory to get an overview of available configuration files
2. Read the main context/configuration file (e.g., CONTEXT.md) to extract key settings
3. Search for specific configuration values within the context file using grep
4. Check the git log to understand recent changes and commits
5. Compile the discovered information into a coherent summary

## Tools Used
- exec: to run shell commands for directory listing, file reading, searching, and git history