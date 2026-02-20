# Deploy Code Changes with Cross-Branch Git Management
Modify source code, verify functionality, commit changes, and cherry-pick across git branches while preserving uncommitted work.

## When to Use
When you need to make a code change in a feature branch but also need to apply it to main, and you want to preserve any other work-in-progress changes in your current branch.

## Steps
1. Read the target source file to understand its current state
2. Edit the file with the necessary changes
3. Verify the change doesn't break imports/syntax by running a Python import test
4. Restart the affected system service and confirm it's running
5. Commit the changes with a descriptive message that includes justification
6. Check your current branch name
7. Stash any uncommitted changes, switch to main, cherry-pick the commit, push to remote, return to original branch, and restore stashed changes

## Tools Used
- read: to examine the source file before modification
- edit: to apply code changes to the target file
- exec: to run Python import verification, manage systemd services, and execute git operations
