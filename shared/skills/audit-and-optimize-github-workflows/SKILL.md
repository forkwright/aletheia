# Audit and Optimize GitHub Workflows
Systematically review all GitHub workflow files, identify optimization opportunities, and apply improvements across the workflow suite.

## When to Use
When you need to audit CI/CD configurations for efficiency improvements, reduce redundant triggers, optimize caching strategies, or consolidate workflow logic across multiple files in a repository.

## Steps
1. Discover all workflow files in `.github/workflows/` and related config files using find with pattern matching
2. Read each workflow file to understand current configuration, triggers, jobs, and dependencies
3. Examine related configuration files (e.g., `dependabot.yml`, `package.json`, test configs) to understand the full context
4. Check recent workflow execution history using GitHub CLI to identify patterns and pain points
5. Review test configuration files to understand test tiers and optimization opportunities
6. Plan optimizations (e.g., shared caching, conditional test execution, trigger path filtering)
7. Update workflow files with improvements
8. Review changes with git diff to verify correctness
9. Commit changes with descriptive message explaining the optimizations made

## Tools Used
- exec: find workflow files, list recent runs with gh CLI, review git changes
- read: examine workflow YAML files and related configuration files
- write: update workflow files with optimized configuration
- exec (git): stage and commit changes with meaningful messages
