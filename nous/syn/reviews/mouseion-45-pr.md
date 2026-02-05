# Mouseion Issue #45 - TVDB API Integration

## PR Created
**PR #153**: https://github.com/forkwright/mouseion/pull/153

## Summary
Implemented TVDB v4 API integration for TV show metadata fetching.

## Files Changed (4 files, +1151/-55 lines)

### New Files
1. **`src/Mouseion.Core/MetadataSource/TVDB/TVDBClient.cs`** (175 lines)
   - Low-level HTTP client for TVDB v4 API
   - JWT authentication with automatic token caching (23h expiry)
   - Polly retry policy with exponential backoff
   - Thread-safe token refresh using SemaphoreSlim

2. **`tests/Mouseion.Core.Tests/MetadataSource/TVDBClientTests.cs`** (165 lines)
   - Tests for authentication flow
   - Token caching verification
   - Error handling tests (missing API key, auth failures, HTTP errors)
   - Request header/URL construction tests

3. **`tests/Mouseion.Core.Tests/MetadataSource/TVDBProxyTests.cs`** (340 lines)
   - Comprehensive tests for all 3 API methods
   - JSON parsing edge cases
   - Caching behavior verification
   - Pagination handling tests

### Modified Files
4. **`src/Mouseion.Core/MetadataSource/TVDB/TVDBProxy.cs`** (refactored from 55 to 435 lines)
   - Replaced TODO placeholders with full implementation
   - Uses ITVDBClient for HTTP calls (dependency injection)
   - Memory caching with 15-minute expiry
   - Robust JSON parsing with fallbacks

## Implementation Details

### Authentication
- POST to `/login` with API key
- Returns JWT token valid for 24 hours
- Token cached for 23 hours (refresh before expiry)
- Thread-safe token acquisition

### API Methods
| Method | Endpoint | Features |
|--------|----------|----------|
| `GetSeriesByTvdbIdAsync` | `/series/{id}/extended` | Full metadata with remote IDs |
| `GetEpisodesBySeriesIdAsync` | `/series/{id}/episodes/default` | Pagination support (50 page limit) |
| `SearchSeriesAsync` | `/search?query=&type=series` | URL encoding, type filtering |

### Resilience
- Polly retry: 3 attempts with exponential backoff (500ms base)
- Handles HttpRequestException, TaskCanceledException
- Circuit breaker inherited from codebase patterns
- Token refresh on 401 Unauthorized

## Commit
```
feat(tv): implement TVDB v4 API integration

- Add TVDBClient with JWT authentication and Polly retry policies
- Implement GetSeriesByTvdbIdAsync with series metadata parsing
- Implement GetEpisodesBySeriesIdAsync with pagination support
- Implement SearchSeriesAsync with type filtering
- Add memory caching for API responses
- Map TVDB status, genres, artwork, and remote IDs (IMDB/TMDB)
- Add comprehensive unit tests for TVDBClient and TVDBProxy

Closes #45
```

## Notes
- dotnet CLI not available on this machine; build verification deferred to CI
- Pre-existing uncommitted changes in repo were intentionally excluded from this commit
- Tests use Moq for mocking IHttpClient and ITVDBClient interfaces
