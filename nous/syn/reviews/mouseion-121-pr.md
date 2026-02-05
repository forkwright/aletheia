# Mouseion Issue #121 - LoggerMessage Migration

## PR Created
- **PR #154**: https://github.com/forkwright/mouseion/pull/154
- **Branch**: `feature/121-logger-message`
- **Base**: `develop`
- **Status**: Partial implementation

## Summary
Migrating logging calls from direct `_logger.LogXxx()` to `[LoggerMessage]` source generator pattern for compile-time optimization.

## What Was Completed

### Files Migrated (25 total)

#### Comic Module (3 files)
- `AddComicSeriesService.cs`
- `ComicVine/ComicVineClient.cs`
- `RefreshComicSeriesService.cs`

#### Manga Module (4 files)
- `AddMangaSeriesService.cs`
- `AniList/AniListClient.cs`
- `MangaDex/MangaDexClient.cs`
- `RefreshMangaSeriesService.cs`

#### Notifications Module (7 files)
- `NotificationService.cs`
- `Discord/Discord.cs`
- `Telegram/Telegram.cs`
- `Slack/Slack.cs`
- `Email/Email.cs`
- `Gotify/Gotify.cs`
- `Apprise/Apprise.cs`

#### Music Module (3 files)
- `AddArtistService.cs`
- `AddAlbumService.cs`
- `AddTrackService.cs`

#### TV Module (1 file)
- `AddSeriesService.cs`

#### Podcasts Module (2 files)
- `AddPodcastService.cs`
- `RSS/RSSFeedParser.cs`

#### News Module (3 files)
- `AddNewsFeedService.cs`
- `RefreshNewsFeedService.cs`
- `RSS/NewsFeedParser.cs`

#### Webcomic Module (1 file)
- `AddWebcomicSeriesService.cs`

#### Subtitles Module (1 file)
- `SubtitleService.cs`

## Remaining Work (41 files)

### API Layer
- `GlobalExceptionHandlerMiddleware.cs`

### HealthCheck
- `DiskSpaceCheck.cs`

### MediaCovers
- `ImageResizer.cs`

### MediaFiles/Import (9 files)
- `Aggregation/AggregationService.cs`
- `FileImportService.cs`
- `ImportApprovedFiles.cs`
- `ImportDecisionMaker.cs`
- `ImportStrategySelector.cs`
- `MediaFileVerificationService.cs`
- `Specifications/AlreadyImportedSpecification.cs`
- `Specifications/HasAudioTrackSpecification.cs`
- `Specifications/MinimumQualitySpecification.cs`
- `Specifications/UpgradeSpecification.cs`

### MediaFiles (4 files)
- `MediaAnalyzer.cs`
- `MediaInfo/MediaInfoService.cs`
- `MediaInfo/UpdateMediaInfoService.cs`
- `MusicFileAnalyzer.cs`
- `MusicFileScanner.cs`

### MetadataSource (7 files)
- `AudiobookInfoProxy.cs`
- `BookInfoProxy.cs`
- `MusicBrainzInfoProxy.cs`
- `PodcastIndexProxy.cs`
- `ResilientMetadataClient.cs`
- `TVDB/TVDBProxy.cs`
- `TVDB/TVDBClient.cs`
- `TmdbInfoProxy.cs`

### Movies (6 files)
- `Calendar/MovieCalendarService.cs`
- `Import/ImportApprovedMovies.cs`
- `Import/MovieImportDecisionMaker.cs`
- `Import/Specifications/HasVideoTrackSpecification.cs`
- `Import/Specifications/MinimumQualitySpecification.cs`
- `Import/Specifications/UpgradeSpecification.cs`
- `Monitoring/ReleaseMonitoringService.cs`
- `Organization/FileOrganizationService.cs`

### Music (3 files)
- `AcoustIDService.cs`
- `MusicQualityParser.cs`
- `MusicReleaseMonitoringService.cs`

### Other
- `Notifications/NotificationFactory.cs`
- `Subtitles/OpenSubtitlesProxy.cs`
- `TV/SceneNumbering/SceneMappingService.cs`
- `Tags/AutoTagging/AutoTaggingService.cs`

## Pattern Applied

```csharp
// Before (evaluated even if logging disabled)
_logger.LogInformation("Added {Title} (ID: {Id})", item.Title, item.Id);

// After (zero-cost when logging disabled)
LogItemAdded(item.Title, item.Id);

[LoggerMessage(Level = LogLevel.Information, Message = "Added {Title} (ID: {Id})")]
private partial void LogItemAdded(string title, int id);
```

### Key Changes
1. Class declaration changed to `partial class`
2. Direct logging calls replaced with partial method calls
3. `[LoggerMessage]` attributes added at end of each class
4. For exception logging, `Exception ex` parameter comes first in the partial method

## Commits
1. `1cc9105` - Comic, Manga, Notifications, Music (17 files)
2. `647ceee` - TV, Podcasts, News, Webcomic (6 files)
3. `adb63a5` - Subtitles, Podcasts RSS (2 files)

## Notes
- This is a **partial implementation** - 25/66 files migrated (~38%)
- No functional changes, purely structural refactoring
- Build verification not possible (no .NET SDK on worker node)
- Files with many logging calls (like `TmdbInfoProxy.cs` with 35 calls) were deferred

## Next Steps
To complete this PR:
1. Continue migrating remaining 41 files
2. Run `dotnet build` to verify compilation
3. Run tests if available
4. Squash commits before merge if desired

---
*Generated: 2025-01-28*
