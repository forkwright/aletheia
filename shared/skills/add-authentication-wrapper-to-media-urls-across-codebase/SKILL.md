# Add Authentication Wrapper to Media URLs Across Codebase

Systematically locate all media URL references in a codebase and wrap them with an authentication function to enable secure image/media loading.

## When to Use
When you need to add authentication to all media URLs (images, audio, video) across a multi-file codebase, ensuring consistent security handling without manual file-by-file inspection.

## Steps
1. Search recursively for all media URL field names and patterns (e.g., `coverArtUrl`, `imageUrl`, `coverUrl`, `poster`) across the codebase using grep with file type filters
2. Identify the authentication wrapper function and its import location (e.g., `authenticateUrl` from `api/client`)
3. Locate all files that reference media URL assignments or usage patterns
4. For each file, add the import statement for the authentication wrapper function
5. Find all instances where media URLs are assigned to variables or used directly in JSX/HTML attributes
6. Wrap the URL value with the authentication function (e.g., `src={imageUrl}` becomes `src={authenticateUrl(imageUrl)}`)
7. Handle conditional cases where URLs might be null/undefined by wrapping inside the condition
8. Build and run tests to verify changes don't break functionality
9. Deploy the updated codebase

## Tools Used
- exec: Search for all media URL references and patterns across the codebase
- read: Preview file content to understand context before editing
- edit: Update import statements and wrap URLs with authentication function
- exec: Run build and test commands to validate changes
