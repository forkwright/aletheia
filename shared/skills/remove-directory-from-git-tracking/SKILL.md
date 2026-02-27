# Remove Directory from Git Tracking
Remove a directory from git version control while keeping it locally, updating .gitignore if needed.

## When to Use
When you need to stop tracking a directory in git (e.g., local-only configuration, temporary files, or sensitive data) while preserving it in the local filesystem and preventing future commits from including it.

## Steps
1. Verify the directory is listed in .gitignore to confirm it should be untracked
2. List all files currently tracked in git under that directory to understand scope
3. Use `git rm -r --cached` to remove the directory from git's index without deleting local files
4. Commit the changes with a descriptive message explaining why the directory is being removed from tracking
5. Push the commit to the remote repository

## Tools Used
- exec: used to run git commands (cat, ls-files, rm, commit, push) and shell operations (grep)