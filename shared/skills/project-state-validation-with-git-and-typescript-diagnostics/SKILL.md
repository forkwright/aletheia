# Project State Validation with Git and TypeScript Diagnostics

Validate project execution state and identify compilation errors after code changes.

## When to Use
When you need to verify that a project is in a valid state after modifications, understand what files changed, and diagnose TypeScript compilation issues before proceeding with further work.

## Steps
1. Check project execution status using plan_execute with a status action to understand the current phase and state
2. Run git status to see which files have been modified
3. Run git diff --stat to see the scope of changes across files
4. Attempt TypeScript compilation check from the project root
5. If initial compilation fails or gives unexpected output, navigate to the specific runtime directory and run TypeScript compiler again
6. Count compilation errors to quantify the severity of issues
7. Review error messages to identify specific type mismatches or validation problems

## Tools Used
- plan_execute: to check the current project execution state and active phase
- exec (git status): to identify modified files in the working directory
- exec (git diff): to quantify the extent of changes
- exec (tsc): to validate TypeScript compilation and identify type errors
- exec (head/wc): to limit and count diagnostic output for better readability
