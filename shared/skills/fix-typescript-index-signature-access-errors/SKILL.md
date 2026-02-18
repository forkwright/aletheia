# Fix TypeScript Index Signature Access Errors
Resolve TS4111 errors when accessing properties from index signatures in TypeScript by using bracket notation instead of dot notation.

## When to Use
When TypeScript compiler reports "Property comes from an index signature, so it must be accessed with ['propertyName']" errors during type checking, and the code uses dot notation to access these properties.

## Steps
1. Run TypeScript compiler to identify index signature access errors (`npx tsc --noEmit`)
2. Locate the problematic lines in the error output
3. Find the schema definition where the property is defined with an index signature
4. Replace dot notation access (e.g., `nous.maxToolLoops`) with bracket notation (e.g., `nous["maxToolLoops"]`)
5. Use sed or similar tools to perform the replacement in the source file
6. Re-run TypeScript compiler to verify errors are resolved
7. Run relevant test suites to ensure functionality is preserved

## Tools Used
- exec: Run TypeScript compiler and sed commands to identify and fix the errors
- grep: Search for property definitions and usage patterns
- sed: Replace problematic dot notation with bracket notation
- tsc: Validate the fixes compile correctly
- test runners: Verify functionality after fixes
