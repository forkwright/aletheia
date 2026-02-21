# API Access with Authentication Fallback

Diagnose and resolve API authentication issues by attempting unauthenticated access, then retry with bearer token credentials.

## When to Use
When querying an API endpoint that may require authentication, and you need to handle authorization failures gracefully and recover with proper credentials.

## Steps
1. Execute an unauthenticated API request to the target endpoint
2. Check if the response indicates an "Unauthorized" or authentication error
3. If authentication is required, retry the same request with a Bearer token in the Authorization header
4. Parse and format the successful response for readability (e.g., using json.tool)

## Tools Used
- exec: Execute curl commands to make HTTP requests and handle authentication workflows