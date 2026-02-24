# Credential Discovery and Authentication System Investigation
Locate and analyze authentication mechanisms, credential storage, and API error handling in a codebase.

## When to Use
When you need to understand how a system handles credentials, authentication tokens, and API authentication flows, especially to diagnose authentication-related issues or trace error handling paths.

## Steps
1. Search for authentication-related keywords (oauth, token, credentials, refresh_token) across the codebase
2. Explore directory structure to locate authentication-related modules
3. Identify credential storage locations (config files, environment paths)
4. Read key authentication handler files to understand the auth flow
5. Locate and examine error handling classes for provider/authentication errors
6. Search for rate-limit and error-related patterns in router/handler code
7. Check for credential configuration files in standard locations (~/.aletheia/credentials/)
8. Trace authentication flows through setup/auth routes
9. Search UI authentication code for client-side credential handling
10. Use web search/fetch to understand standard API error response formats if needed

## Tools Used
- grep: Search for authentication keywords across multiple file types and paths
- find: Locate TypeScript files in specific directories
- exec: List directory contents and read credential files
- read: Examine authentication handler and error class implementations
- web_search: Query external documentation for API error standards
- web_fetch: Retrieve official API documentation on error handling
