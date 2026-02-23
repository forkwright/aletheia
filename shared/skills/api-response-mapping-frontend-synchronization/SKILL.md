# API Response Mapping & Frontend Synchronization

Diagnose and fix mismatches between API response field names and frontend expectations by creating a mapping layer, applying it consistently across all endpoints, and adding comprehensive error logging.

## When to Use

When frontend UI shows blank screens, zero values, or runtime errors after API integration, particularly when:
- API returns fields with different names than frontend expects (e.g., `durationSeconds` vs `duration`, `artistName` vs `artist`)
- Multiple endpoints return the same object types but frontend expects normalized data
- Error handling is missing or inconsistent across store methods
- Need to maintain backward compatibility while fixing data flow

## Steps

1. **Diagnose the mismatch**: Query API schema (via Swagger/OpenAPI) and compare field names against frontend type definitions
2. **Create mapping functions**: Write pure functions (e.g., `mapTrack()`, `mapAlbum()`, `mapArtist()`) that transform raw API responses to frontend types
3. **Create a result mapper**: Write a generic `mapPagedResult<T>()` function to apply transformation to paginated responses
4. **Apply mapping to all endpoints**: Update every API client method that returns the affected types to call the mapping function
5. **Add error logging**: Import error logger and wrap try-catch blocks to call `logError()` before state updates
6. **Update store methods**: Ensure Zustand store async actions use the mapped responses and consistent error handling
7. **Create error boundary**: Add React error boundary component wrapping app routes
8. **Test thoroughly**: Run unit tests on mapping functions and integration tests on affected pages
9. **Commit and deploy**: Create PR with all changes, squash-merge, and redeploy frontend

## Tools Used

- exec: diagnose API schema, verify field names, run tests, build and deploy
- read/grep: inspect existing code patterns and identify all affected methods
- edit: modify API client methods, store methods, add error logging, wrap components
- write: create new mapping functions, error boundary component, memory docs
- note: track fix progress and current state across tool calls
