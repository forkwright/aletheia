# Document Specification Approval and Version Control Workflow
Update a draft specification document with approval metadata, then commit changes to version control.

## When to Use
When a specification or design document has been reviewed and approved, and needs to be locked into the project history with proper metadata and git tracking.

## Steps
1. Read the current specification document to understand its content and structure
2. Update the document content to change status from "Draft" to "Approved" and add reviewer information
3. Write the updated content back to the document file
4. Commit the changes to git with a descriptive message that summarizes the locked decisions

## Tools Used
- read: Retrieve the current specification document content
- write: Update the document with approval status and reviewer metadata
- exec: Execute git commands to stage and commit the document changes with meaningful commit message