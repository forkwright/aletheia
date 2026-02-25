# Audit Timestamp and Date Handling Across Codebase

Systematically locate all timestamp/date references in frontend components, backend services, and database schemas to identify inconsistencies or audit temporal data handling patterns.

## When to Use
When you need to:
- Audit how timestamps are created, formatted, and stored across a full-stack application
- Investigate timezone handling or date formatting issues
- Identify all locations where temporal data is referenced before refactoring
- Understand the current timestamp strategy (frontend formatting vs. backend generation vs. database defaults)

## Steps
1. Search frontend components (Svelte files) for timestamp/date formatting patterns using common keywords (toLocale, Intl.Date, formatTime, etc.)
2. Search TypeScript services for timestamp creation and manipulation patterns
3. Search chat/UI components for time display logic
4. Read utility/library files that contain formatting functions
5. Search backend runtime files for date/timestamp handling in tests and actual code
6. Search database schema files for timestamp column definitions and DEFAULT values
7. Check database test files to see how timestamps are being mocked or manipulated
8. Verify system timezone configuration and current time settings

## Tools Used
- grep: Pattern matching across multiple file types (*.svelte, *.ts) to find timestamp-related code
- read: Examining utility/formatting libraries to understand how dates are being processed
- exec: Checking system timezone configuration and current time to identify timezone context
