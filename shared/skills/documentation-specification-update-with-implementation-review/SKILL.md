# Documentation Specification Update with Implementation Review

Update technical specification documents by cross-referencing implementation code, then commit changes to version control.

## When to Use
When you need to update specification documents based on actual implementation details, ensure consistency between specs and code, and track changes in git.

## Steps
1. Read the specification document to understand current status and structure
2. Read the actual implementation file to extract current details and behavior
3. Edit the specification document to incorporate implementation findings and update phase/status information
4. Edit related index or summary documents to reflect the status change
5. Check git staging status to verify which files changed
6. Commit the documentation changes with a descriptive message referencing the specification and phase

## Tools Used
- read: to access specification and implementation files
- edit: to update documentation with new information and status
- exec: to run git commands for staging and committing changes
