# Missing Tool Diagnosis and Location Discovery
Systematically locate unavailable command-line tools by checking PATH, searching the filesystem, and examining relevant binary directories.

## When to Use
When an expected command-line tool is not found in the standard PATH and you need to determine if it's installed elsewhere in the system or is completely missing.

## Steps
1. Attempt to run the missing command to confirm it's not available
2. Check system health/status if applicable to understand the environment state
3. Use `which` command to search PATH for the tool
4. List environment-specific binary directories (e.g., $ALETHEIA_SHARED/bin) to see available tools
5. Use `find` to search the filesystem for the missing tool by name
6. Check domain-specific tool directories (e.g., /mnt/ssd/aletheia/shared/bin/) that may contain project-specific binaries
7. Compare available tools in relevant directories against the requested tool to determine if it exists elsewhere

## Tools Used
- exec: Run shell commands to test command availability, check paths, and search filesystem
- which: Locate commands in PATH
- find: Search filesystem for tools by name
- ls: List directory contents to discover available binaries in specific locations
