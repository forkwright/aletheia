# Locate and Inspect Project Metadata in Hierarchical Directory Structure

Navigate a nested project directory structure to find and read project configuration and status files.

## When to Use
When you need to discover project locations within a complex codebase, understand project state and configuration, or locate specific documentation files organized in a hierarchical directory with ID-based naming conventions (e.g., `.dianoia/projects/proj_*/`).

## Steps
1. Search for known configuration file patterns (e.g., "PROJECT.md") in a root search path to understand the structure
2. If not found at root, locate the configuration directory (e.g., `.dianoia`)
3. List the projects directory to discover available projects
4. List individual project folders to identify project IDs
5. Read the PROJECT.md file from the discovered project to obtain project metadata
6. List phase/component subdirectories to understand project structure
7. Read additional configuration files (ROADMAP.md, REQUIREMENTS.md) as needed for complete context

## Tools Used
- find: Locate configuration files by pattern matching
- ls: List directory contents to discover projects and subdirectories
- read: Extract metadata and documentation from discovered configuration files
- grep: Search for specific patterns in files when targeted location is unknown
