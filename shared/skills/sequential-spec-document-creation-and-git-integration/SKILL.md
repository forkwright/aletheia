# Sequential Spec Document Creation and Git Integration

Creates a new numbered specification document following an existing naming convention, writes it to a structured directory, and commits it to version control with appropriate metadata.

## When to Use
When you need to add a new entry to a numbered series of specification or documentation files, maintain consistency with existing numbering and formatting conventions, and ensure changes are tracked in git with descriptive commit messages.

## Steps
1. Query the target directory to identify the latest numbered document and establish the next sequence number
2. Create a new document with proper header metadata (status, author, date) following the established template
3. Write the document to the appropriate path with the next sequential number
4. Stage the new file in git, commit with a descriptive message referencing the spec number and topic, and push to the remote repository

## Tools Used
- exec: to list existing files with sorting to find the latest sequence number, and to execute git commands (add, commit, push)
- write: to create the new specification document with full content and metadata at the target path
