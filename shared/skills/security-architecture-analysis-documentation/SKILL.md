# Security Architecture Analysis & Documentation
Investigate authentication mechanisms in a codebase and document security design decisions.

## When to Use
When you need to understand the current authentication/security implementation, identify gaps, and create or update security specifications for a system.

## Steps
1. Extract current version information from package metadata
2. Check existing git tags and version history
3. Review existing security-related specification documents
4. Search codebase for authentication-related keywords (auth, jwt, scrypt, session, password, hash)
5. Examine all relevant authentication source files in sequence (passwords, sessions, middleware, tokens)
6. Check configuration schemas for auth modes and options
7. Verify system service files and deployment configuration
8. Trace entry points and CLI bootstrap code
9. Write a new specification document synthesizing findings
10. Commit the specification to version control with descriptive message

## Tools Used
- exec: grep for auth-related code patterns, view specific files/line ranges, check service configs, verify binary locations
- write: create new specification document with findings
- git: commit specification changes with meaningful messages
