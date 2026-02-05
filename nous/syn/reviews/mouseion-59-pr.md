# Mouseion Issue #59 - OpenTelemetry Support

**PR**: https://github.com/forkwright/mouseion/pull/157
**Branch**: `feature/59-opentelemetry` → `develop`
**Status**: Created

## Summary

Implemented comprehensive OpenTelemetry instrumentation for production observability in the Mouseion media manager.

## Changes Made

### New Files
1. **`src/Mouseion.Common/Instrumentation/MouseionMetrics.cs`** - Centralized metrics collection class with:
   - API metrics (requests, errors, latency histograms)
   - Media operations metrics (imports, downloads, transcodes)
   - Metadata provider metrics (TMDb, MusicBrainz, etc.)
   - Download client metrics
   - Database query metrics
   - Cache hit/miss/eviction metrics
   - Background job metrics
   - Library item gauges
   - Indexer search metrics
   - Tracing helper methods for creating spans

2. **`src/Mouseion.Api/Middleware/TelemetryMiddleware.cs`** - ASP.NET Core middleware for:
   - Automatic request timing
   - Normalized endpoint labeling (prevents metric cardinality explosion)
   - Error categorization
   - Span attribute enrichment

3. **`tests/Mouseion.Common.Tests/Instrumentation/MouseionMetricsTests.cs`** - Unit tests for metrics
4. **`tests/Mouseion.Common.Tests/Instrumentation/OpenTelemetryConfigurationTests.cs`** - Unit tests for configuration

5. **`docs/observability/README.md`** - Comprehensive documentation covering:
   - Configuration options
   - Available metrics reference
   - Distributed tracing setup
   - Prometheus integration with PromQL examples
   - Jaeger integration
   - Grafana dashboard setup

6. **`docs/observability/grafana-dashboard.json`** - Ready-to-import Grafana dashboard with panels for:
   - API request rates and latency percentiles
   - Error rates
   - Active jobs
   - Cache hit rates
   - Library item counts
   - Metadata provider performance
   - Database query metrics
   - Background job execution

### Modified Files
1. **`src/Mouseion.Common/Mouseion.Common.csproj`** - Added packages:
   - `OpenTelemetry.Exporter.OpenTelemetryProtocol`
   - `OpenTelemetry.Exporter.Prometheus.AspNetCore`

2. **`src/Mouseion.Host/Mouseion.Host.csproj`** - Added same exporter packages

3. **`src/Mouseion.Common/Instrumentation/OpenTelemetryConfiguration.cs`** - Enhanced with:
   - `TelemetryOptions` configuration class
   - Prometheus exporter support
   - OTLP exporter support for Jaeger/Tempo
   - Configurable resource attributes
   - ASP.NET Core instrumentation enrichment
   - Helper methods for creating typed spans

4. **`src/Mouseion.Common/Instrumentation/DiagnosticsContext.cs`** - Updated to use MouseionMetrics (backward compatibility)

5. **`src/Mouseion.Host/Program.cs`** - Added:
   - TelemetryMiddleware to pipeline
   - Prometheus endpoint mapping
   - Configuration pass-through to telemetry setup

6. **`src/Mouseion.Host/appsettings.json`** - Added Telemetry configuration section

## Key Features

### Metrics (all with appropriate labels/dimensions)
- `mouseion_api_requests_total` - Counter with endpoint, method, status_code
- `mouseion_api_request_duration_milliseconds` - Histogram for latency percentiles
- `mouseion_media_imports_total` - Media import operations
- `mouseion_metadata_requests_total` - Metadata provider calls
- `mouseion_database_queries_total` - Database query counts
- `mouseion_cache_hits_total` / `mouseion_cache_misses_total` - Cache performance
- `mouseion_jobs_total` - Background job executions
- `mouseion_library_items` - Gauge of library contents

### Distributed Tracing
- Automatic span creation for HTTP requests
- Database operation spans with db.* semantic conventions
- Background job spans
- External API call spans with provider attribution
- Error recording with exception details

### Configuration Options
```json
{
  "Telemetry": {
    "Enabled": true,
    "EnablePrometheus": true,
    "EnableConsoleExporter": false,
    "OtlpEndpoint": "http://jaeger:4317",
    "ServiceInstanceId": "instance-1",
    "ResourceAttributes": {
      "deployment.environment": "production"
    }
  }
}
```

## Acceptance Criteria Met
- ✅ Custom metrics emitted for all major operations
- ✅ Distributed tracing across all service calls
- ✅ Prometheus exporter configured (at `/metrics`)
- ✅ Example Grafana dashboard provided
- ✅ Documentation for observability setup

## Notes
- Prometheus exporter version is `1.14.0-rc.1` (release candidate) as stable release not yet available
- Endpoint normalization prevents metric cardinality issues by replacing dynamic IDs with `{id}`
- Health and metrics endpoints are excluded from tracing to reduce noise
- Tests don't require a running OpenTelemetry collector - they verify instrumentation code paths
