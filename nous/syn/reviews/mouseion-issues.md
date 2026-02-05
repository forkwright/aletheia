# Mouseion Open Issues Review

**Repository:** forkwright/mouseion  
**Review Date:** 2025-07-19  
**Total Open Issues:** 10

---

## Summary

| Issue | Title | Complexity | Priority | Est. Effort |
|-------|-------|------------|----------|-------------|
| #121 | LoggerMessage migration | Low | Low | 2-3 days |
| #93 | Comprehensive integration tests | Medium | Medium | 1-2 weeks |
| #90 | Test coverage expansion | High | High | 3-4 weeks |
| #61 | Podcast transcription (Whisper) | High | Low | 2-3 weeks |
| #60 | Smart playlist generation | Medium | Low | 1 week |
| #59 | OpenTelemetry instrumentation | Medium | Medium | 1-2 weeks |
| #58 | Multi-zone WebSocket playback | Very High | Medium | 4-6 weeks |
| #57 | Taste profile analytics | High | Deferred | 3-4 weeks |
| #45 | TVDB API implementation | Low-Medium | Medium | 2-3 days |
| #39 | Bulk operations API | Medium | Medium | 3-5 days |

---

## Detailed Analysis

### #121 - LoggerMessage Migration (CA1873)

**Complexity:** Low  
**Priority:** Low  
**Estimated Effort:** 2-3 days

**Problem:** 337 logging calls use string interpolation, causing performance overhead even when logging is disabled.

**Implementation Approach:**
1. **Tooling:** Create a Roslyn analyzer script or regex pattern to identify all interpolated logging calls
2. **Pattern:** For each call site, create a `[LoggerMessage]` partial method in the containing class
3. **Batching:** Process by namespace/folder to maintain focus (e.g., Controllers first, then Services)
4. **Testing:** Verify log output format matches before/after

**Suggested Implementation:**
```csharp
// Create a shared partial class per file with all LoggerMessage methods
public partial class MovieController
{
    [LoggerMessage(Level = LogLevel.Debug, Message = "Processing {ItemId} for user {UserId}")]
    partial void LogProcessing(int itemId, int userId);
}
```

**Recommendation:** Good candidate for a junior dev or as a refactoring sprint. Could be automated with a simple script. Low risk, incremental commits per folder.

---

### #93 - Comprehensive Integration Test Scenarios

**Complexity:** Medium  
**Priority:** Medium  
**Estimated Effort:** 1-2 weeks

**Problem:** Integration tests only cover happy paths; no negative scenarios, authorization failures, or edge cases.

**Implementation Approach:**
1. **Inventory existing tests:** Map which endpoints have coverage
2. **Template-based generation:** Create test templates for each response type (401, 403, 404, 409, 422)
3. **Shared fixtures:** Build reusable test data factories and auth helpers
4. **Categories:** Implement tests in phases:
   - Phase 1: Authorization tests (401/403)
   - Phase 2: Resource not found (404)
   - Phase 3: Conflicts and validation (409, 422)
   - Phase 4: Edge cases (pagination, empty results)
   - Phase 5: Relationship integrity (cascades)

**Test Structure:**
```csharp
public class MoviesApiNegativeTests : IClassFixture<MouseionWebFactory>
{
    [Fact] public async Task Get_NonExistent_Returns404()
    [Fact] public async Task Post_NoAuth_Returns401()
    [Fact] public async Task Post_Duplicate_Returns409()
    [Theory] 
    [InlineData(0), InlineData(-1), InlineData(10001)]
    public async Task Get_InvalidPageSize_Returns422(int pageSize)
}
```

**Recommendation:** Tackle alongside #90. Can be parallelized across team members (each person owns a controller's negative tests).

---

### #90 - Test Coverage Expansion (95% Untested)

**Complexity:** High  
**Priority:** HIGH  
**Estimated Effort:** 3-4 weeks

**Problem:** Only 12 test files for 495 source files. Critical systems completely untested.

**Implementation Approach:**
1. **Add mocking library first:** NSubstitute recommended (cleaner syntax than Moq)
   ```bash
   dotnet add package NSubstitute
   ```
2. **Prioritize by risk:**
   - **Week 1:** Notification system (25 files) - high user impact
   - **Week 2:** Media file processing (30+ files) - core functionality
   - **Week 3:** Import lists (23 files) - external integrations
   - **Week 4:** History/tracking + Health endpoint

3. **Coverage tooling:** Add Coverlet + ReportGenerator to CI
   ```xml
   <PackageReference Include="coverlet.collector" Version="6.0.0" />
   ```

4. **Minimum threshold:** Start at 40%, increment by 10% per sprint until 80%

**Quick wins first:**
- Health endpoint (critical for k8s, ~2 hours)
- Simple service classes with few dependencies

**Recommendation:** This is foundational work. Block new features until baseline coverage exists. High priority.

---

### #61 - Podcast Transcription with Whisper

**Complexity:** High  
**Priority:** Low  
**Estimated Effort:** 2-3 weeks

**Problem:** Podcasts lack searchable text, limiting discoverability.

**Implementation Approach:**
1. **Architecture decision:** Start with OpenAI API (faster to implement), make provider swappable
2. **Database schema:**
   ```sql
   CREATE TABLE PodcastTranscripts (
       Id INT PRIMARY KEY,
       EpisodeId INT REFERENCES PodcastEpisodes(Id),
       StartTimeMs INT,
       EndTimeMs INT,
       Text NVARCHAR(MAX),
       Confidence FLOAT,
       CreatedAt DATETIME2
   );
   CREATE FULLTEXT INDEX ON PodcastTranscripts(Text);
   ```
3. **Background job:** Queue transcription on episode download, process async
4. **Cost management:** Add configuration for auto-transcription (on/off), max duration limits

**Implementation phases:**
- Phase 1: OpenAI Whisper API integration + manual trigger
- Phase 2: Automatic transcription on import
- Phase 3: Full-text search endpoint
- Phase 4: (Optional) Self-hosted Whisper fallback

**Recommendation:** Defer until core features stable. Nice-to-have but not essential. Consider as a plugin/extension rather than core feature.

---

### #60 - Smart Playlist Generation

**Complexity:** Medium  
**Priority:** Low  
**Estimated Effort:** 1 week

**Problem:** Database schema exists (Migration 012) but service layer not implemented.

**Implementation Approach:**
1. **Rule engine:** Use expression trees or a simple DSL for rule evaluation
   ```csharp
   public class PlaylistRule
   {
       public string Field { get; set; }     // "tempo", "dr", "key"
       public string Operator { get; set; }  // "gt", "lt", "eq", "between"
       public object Value { get; set; }
   }
   ```

2. **Service implementation:**
   ```csharp
   public class SmartPlaylistService : ISmartPlaylistService
   {
       public async Task<List<Track>> EvaluatePlaylist(SmartPlaylist playlist)
       {
           var query = _dbContext.Tracks.AsQueryable();
           foreach (var rule in playlist.Rules)
               query = ApplyRule(query, rule);
           return await query.ToListAsync();
       }
   }
   ```

3. **Auto-refresh:** Use database triggers or a scheduled job to re-evaluate on library changes

**Presets to include:**
- Workout (tempo: 120-140 BPM, energy: high)
- Chill (tempo: <90 BPM, DR: >10)
- Discovery (added: last 30 days, playCount: 0)
- Audiophile (DR: >12, format: lossless)

**Recommendation:** Low-hanging fruit since schema exists. Good feature for engagement.

---

### #59 - OpenTelemetry Metrics and Tracing

**Complexity:** Medium  
**Priority:** Medium  
**Estimated Effort:** 1-2 weeks

**Problem:** OpenTelemetry installed but not fully utilized.

**Implementation Approach:**
1. **Custom metrics via `System.Diagnostics.Metrics`:**
   ```csharp
   public static class MouseionMetrics
   {
       public static readonly Meter Meter = new("Mouseion", "1.0");
       public static readonly Counter<int> ImportsTotal = Meter.CreateCounter<int>("mouseion_imports_total");
       public static readonly Histogram<double> ApiLatency = Meter.CreateHistogram<double>("mouseion_api_latency_ms");
   }
   ```

2. **Instrument key operations:**
   - API middleware (latency, status codes)
   - Download client operations
   - Metadata provider calls
   - Database queries (via EF Core interceptor)

3. **Tracing spans:** Use `Activity` API for distributed tracing
   ```csharp
   using var activity = ActivitySource.StartActivity("FetchMetadata");
   activity?.SetTag("provider", "tmdb");
   ```

4. **Export configuration:**
   ```csharp
   services.AddOpenTelemetry()
       .WithMetrics(b => b.AddPrometheusExporter())
       .WithTracing(b => b.AddOtlpExporter());
   ```

**Deliverables:**
- Prometheus `/metrics` endpoint
- Sample Grafana dashboard JSON
- Jaeger tracing setup docs

**Recommendation:** Important for production ops. Implement before scaling up users.

---

### #58 - Multi-zone WebSocket Playback

**Complexity:** Very High  
**Priority:** Medium  
**Estimated Effort:** 4-6 weeks

**Problem:** No synchronized playback across devices/zones.

**Implementation Approach:**
1. **WebSocket infrastructure:**
   ```csharp
   app.UseWebSockets();
   app.Map("/api/v3/zones", async context => {
       var ws = await context.WebSockets.AcceptWebSocketAsync();
       await _zoneHub.HandleConnection(ws, context.RequestAborted);
   });
   ```

2. **Zone management:**
   - Registration protocol (zone announces capabilities)
   - Discovery (mDNS/Bonjour for LAN, manual for remote)
   - Grouping (master-slave model for sync)

3. **Sync protocol challenges:**
   - Clock skew compensation (NTP-based timestamps)
   - Latency measurement (round-trip ping)
   - Buffer management (pre-load ~2s on all zones)
   - Drift correction (<50ms target)

4. **Message types:**
   ```json
   { "type": "zone_register", "zoneId": "kitchen", "capabilities": ["audio"] }
   { "type": "sync_play", "trackId": 123, "startAtMs": 1689456789000 }
   { "type": "sync_pause", "pauseAtMs": 45000 }
   ```

**Technical challenges:**
- Network latency variance
- Different device audio buffers
- Reconnection handling mid-playback
- Group leader election if master disconnects

**Recommendation:** This is a substantial project requiring client (Akroasis) coordination. Consider prototyping with 2-zone sync before full implementation. May warrant its own milestone.

---

### #57 - Taste Profile and Listening Analytics

**Complexity:** High  
**Priority:** Deferred  
**Estimated Effort:** 3-4 weeks

**Problem:** No personalized recommendations based on listening patterns.

**Implementation Approach:**
1. **Data collection:** Requires #31 (progress tracking) first
   ```sql
   CREATE TABLE ListeningHistory (
       Id INT PRIMARY KEY,
       TrackId INT,
       UserId INT,
       StartedAt DATETIME2,
       CompletedAt DATETIME2,
       CompletionPercent FLOAT,
       Skipped BIT
   );
   ```

2. **Pattern detection:**
   - Aggregate by time-of-day, day-of-week
   - Calculate skip rate, completion rate
   - Build preference distributions (tempo, key, DR, genre)

3. **Profile generation:**
   ```csharp
   public class TasteProfile
   {
       public Distribution<int> TempoPreference { get; set; }
       public Distribution<string> KeyPreference { get; set; }
       public Range<float> DynamicRangePreference { get; set; }
       public List<string> TopGenres { get; set; }
       public TimePatterns ListeningPatterns { get; set; }
   }
   ```

4. **Privacy:** All local, no external services, user-deletable

**Recommendation:** Defer until progress tracking (#31) is complete. Good Phase 2 feature.

---

### #45 - TVDB API Implementation

**Complexity:** Low-Medium  
**Priority:** Medium  
**Estimated Effort:** 2-3 days

**Problem:** TVDBProxy has 3 TODO placeholders; TV metadata fetching broken.

**Implementation Approach:**
1. **Authentication:** TVDB v4 uses JWT tokens
   ```csharp
   public async Task<string> GetAuthToken()
   {
       var response = await _http.PostAsJsonAsync("https://api4.thetvdb.com/v4/login", 
           new { apikey = _config.ApiKey });
       var result = await response.Content.ReadFromJsonAsync<AuthResponse>();
       return result.Data.Token;
   }
   ```

2. **Implement methods:**
   - `GetSeriesById(int id)` → `GET /v4/series/{id}`
   - `GetEpisodesForSeries(int id)` → `GET /v4/series/{id}/episodes/default`
   - `SearchSeries(string query)` → `GET /v4/search?query={query}&type=series`

3. **Resilience:** Add Polly policies
   ```csharp
   services.AddHttpClient<ITvdbClient>()
       .AddPolicyHandler(GetRetryPolicy())
       .AddPolicyHandler(GetCircuitBreakerPolicy());
   ```

4. **Caching:** Cache series data for 24h (metadata doesn't change often)

**Recommendation:** Straightforward API integration. Good task for getting familiar with the codebase.

---

### #39 - Bulk Operations API

**Complexity:** Medium  
**Priority:** Medium  
**Estimated Effort:** 3-5 days

**Problem:** Library management requires batch operations; individual calls inefficient.

**Implementation Approach:**
1. **Controller endpoints:**
   ```csharp
   [HttpPost("bulk/update")]
   public async Task<ActionResult<BulkResult>> BulkUpdate([FromBody] BulkUpdateRequest request)
   
   [HttpPost("bulk/delete")]
   public async Task<ActionResult<BulkResult>> BulkDelete([FromBody] BulkDeleteRequest request)
   ```

2. **Transaction handling:**
   ```csharp
   await using var transaction = await _dbContext.Database.BeginTransactionAsync();
   try {
       // Validate all IDs exist first
       // Perform updates
       await transaction.CommitAsync();
   } catch {
       await transaction.RollbackAsync();
       throw;
   }
   ```

3. **Validation:**
   - Maximum batch size (100 items)
   - All IDs must exist (pre-validate)
   - Field whitelist for updates

4. **Response format:**
   ```json
   { "updated": 15, "failed": 0 }
   // or with partial success option:
   { "updated": 13, "failed": 2, "errors": [{"id": 5, "reason": "Not found"}] }
   ```

**Design decision:** Atomic (all-or-nothing) vs partial success? Issue specifies atomic. Consider making configurable via query param.

**Recommendation:** Enables better UX in Akroasis. Medium priority, straightforward implementation.

---

## Recommended Priority Order

1. **#90 - Test coverage** (HIGH - foundation for everything else)
2. **#45 - TVDB API** (quick win, unblocks TV functionality)
3. **#39 - Bulk operations** (medium effort, high UX impact)
4. **#59 - OpenTelemetry** (important for production)
5. **#93 - Integration tests** (complements #90)
6. **#121 - LoggerMessage** (low effort, can be done incrementally)
7. **#60 - Smart playlists** (schema exists, moderate effort)
8. **#58 - Multi-zone WebSocket** (large project, needs planning)
9. **#61 - Podcast transcription** (defer until core stable)
10. **#57 - Taste profile** (defer, needs #31 first)

---

*Generated by Clawdbot subagent*
