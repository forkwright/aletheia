# Archive and Update Documentation Index
Move a completed document to an archive folder and update the index file to reflect the change.

## When to Use
When a document or specification has been completed/implemented and needs to be moved to an archive directory, with corresponding updates to an index or README file that tracks active vs. archived items.

## Steps
1. List contents of the archive directory to understand existing structure
2. Move the target file to the archive folder using git mv
3. Update the index/README file to remove the item from the active section
4. Update the index/README file to remove or update any explanatory text about the item
5. Update the index/README file to add the item to the archived section with appropriate metadata
6. Commit all changes with a descriptive message and push to the repository

## Tools Used
- exec: for listing directory contents, moving files with git mv, and committing/pushing changes
- edit: for updating the index/README file to reflect the archival (remove from active, add to archived sections)
