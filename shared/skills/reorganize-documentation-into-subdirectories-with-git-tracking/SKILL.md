# Reorganize Documentation Into Subdirectories with Git Tracking

Restructure scattered documentation files into organized subdirectories, create index documentation, and commit changes to version control.

## When to Use
When you need to:
- Consolidate related documentation files from multiple locations into a single organized subdirectory
- Create an index or README to document the purpose and contents of a documentation collection
- Preserve git history while reorganizing files using `git mv`
- Commit structural documentation changes with descriptive messages

## Steps
1. Explore the current documentation structure using `ls` and `find` to identify files matching a pattern (e.g., `spec-*.md`)
2. Create the target subdirectory using `mkdir -p`
3. Move related files into the subdirectory using `git mv` to preserve history
4. Create an index document (README.md) in the new subdirectory explaining its purpose and contents
5. Stage all changes using `git add`
6. Verify changes with `git status --short`
7. Commit with a descriptive message explaining the reorganization rationale

## Tools Used
- exec: to explore directory structure, create directories, move files with git, and commit changes
- write: to create the index/README document with descriptive content
