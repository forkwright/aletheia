//! Update handlers for the metrics dashboard view.

use std::time::Instant;

use crate::api::types::{CostMetricsResponse, TokenMetricsResponse};
use crate::app::App;
use crate::msg::Msg;
use crate::state::metrics::BackendMetricsSnapshot;
use crate::state::view_stack::View;

/// Re-fetch canonical backend metrics at most this often while the metrics
/// view is open (#4987) — often enough to feel live, rare enough not to
/// hammer the backend from a 16ms UI tick.
const BACKEND_METRICS_REFRESH_TICKS: u64 = 300;

/// Open the metrics dashboard and trigger background health + telemetry fetches.
pub(crate) fn handle_open(app: &mut App) {
    app.layout.view_stack.push(View::Metrics);
    app.layout.metrics.scroll_offset = 0;
    app.layout.metrics.selected_agent = 0;

    // WHY: Fire a detailed health check each time the metrics view opens so the
    // badge reflects current server state rather than the startup snapshot. The
    // check runs in a background task so the update loop never blocks on the
    // HTTP round-trip; the result arrives as Msg::MetricsHealthLoaded.
    let client = app.client.clone();
    app.background_tasks.spawn(async move {
        let result = client.health_details().await;
        Msg::MetricsHealthLoaded(result.map(|r| r.status == "healthy").unwrap_or(false))
    });

    fetch_backend_metrics(app);
}

/// Spawn the background fetch for canonical backend token/cost telemetry.
/// Both calls run independently so one failing does not drop the other's
/// data (#4987's partial-availability requirement).
fn fetch_backend_metrics(app: &mut App) {
    let client = app.client.clone();
    app.background_tasks.spawn(async move {
        let (tokens, costs) = tokio::join!(client.token_metrics(), client.cost_metrics());
        Msg::BackendMetricsLoaded {
            tokens: tokens.map_err(|e| e.to_string()),
            costs: costs.map_err(|e| e.to_string()),
        }
    });
}

/// Periodic refresh while the metrics view is open (#4987's "fetched on a
/// periodic tick" requirement). Call from `handle_tick`; a no-op when the
/// metrics view is not the active view or the interval has not elapsed.
pub(crate) fn maybe_refresh_backend_metrics(app: &mut App) {
    if app.layout.view_stack.current() != &View::Metrics {
        return;
    }
    if !app
        .viewport
        .tick_count
        .is_multiple_of(BACKEND_METRICS_REFRESH_TICKS)
    {
        return;
    }
    fetch_backend_metrics(app);
}

/// Apply the result of a background backend-metrics fetch. Each half is
/// stored independently: a failed `costs` fetch does not discard a
/// successfully-fetched `tokens` snapshot, and vice versa.
pub(crate) fn handle_backend_metrics_loaded(
    app: &mut App,
    tokens: Result<TokenMetricsResponse, String>,
    costs: Result<CostMetricsResponse, String>,
) {
    if let Ok(costs) = &costs {
        // WHY: keeps the always-visible status bar / retrospective cost
        // display fed from the same canonical source, instead of the
        // permanently-zero counter #4987 found there (nothing ever wrote
        // `daily_cost_cents` once the dead `/costs/daily` route was removed).
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "display cents from a USD f64; clamped to u32 range for a dashboard figure"
        )]
        let cents = (costs.today_cost * 100.0)
            .round()
            .clamp(0.0, f64::from(u32::MAX)) as u32;
        app.dashboard.daily_cost_cents = cents;
    }
    app.layout.metrics.backend = Some(BackendMetricsSnapshot {
        tokens,
        costs,
        fetched_at: Instant::now(),
    });
}

/// Close the metrics dashboard and return to the previous view.
pub(crate) fn handle_close(app: &mut App) {
    app.layout.view_stack.pop();
}

/// Move selection up in the per-agent table.
pub(crate) fn handle_select_up(app: &mut App) {
    let metrics = &mut app.layout.metrics;
    if metrics.selected_agent > 0 {
        metrics.selected_agent -= 1;
        if metrics.selected_agent < metrics.scroll_offset {
            metrics.scroll_offset = metrics.selected_agent;
        }
    }
}

/// Move selection down in the per-agent table.
pub(crate) fn handle_select_down(app: &mut App) {
    let count = app.dashboard.agents.len();
    if count == 0 {
        return;
    }
    let metrics = &mut app.layout.metrics;
    if metrics.selected_agent + 1 < count {
        metrics.selected_agent += 1;
        // NOTE: visible_height is an approximation; exact paging is handled by render.
        // We keep the scroll window trailing the cursor.
        if metrics.selected_agent >= metrics.scroll_offset + 20 {
            metrics.scroll_offset = metrics.selected_agent.saturating_sub(19);
        }
    }
}

/// Apply the result of an async health check.
pub(crate) fn handle_health_loaded(app: &mut App, healthy: bool) {
    app.layout.metrics.api_healthy = Some(healthy);
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;
    use crate::app::test_helpers::test_app;

    #[test]
    fn handle_close_pops_view() {
        let mut app = test_app();
        app.layout.view_stack.push(View::Metrics);
        handle_close(&mut app);
        assert_eq!(app.layout.view_stack.current(), &View::Home);
    }

    #[test]
    fn handle_select_up_saturates_at_zero() {
        let mut app = test_app();
        app.layout.metrics.selected_agent = 0;
        handle_select_up(&mut app);
        assert_eq!(app.layout.metrics.selected_agent, 0);
    }

    #[test]
    fn handle_health_loaded_sets_flag() {
        let mut app = test_app();
        handle_health_loaded(&mut app, true);
        assert_eq!(app.layout.metrics.api_healthy, Some(true));
        handle_health_loaded(&mut app, false);
        assert_eq!(app.layout.metrics.api_healthy, Some(false));
    }

    fn sample_tokens() -> TokenMetricsResponse {
        TokenMetricsResponse {
            series: Vec::new(),
            agents: Vec::new(),
            models: Vec::new(),
            today_input: 100,
            today_output: 50,
            week_input: 0,
            week_output: 0,
            month_input: 0,
            month_output: 0,
            prev_today_input: 0,
            prev_today_output: 0,
            prev_week_input: 0,
            prev_week_output: 0,
            prev_month_input: 0,
            prev_month_output: 0,
        }
    }

    fn sample_costs(today_cost: f64) -> CostMetricsResponse {
        CostMetricsResponse {
            series: Vec::new(),
            agents: Vec::new(),
            today_cost,
            week_cost: 0.0,
            month_cost: 0.0,
            prev_today_cost: 0.0,
            prev_week_cost: 0.0,
            prev_month_cost: 0.0,
            data_unavailable: Vec::new(),
        }
    }

    #[test]
    fn handle_backend_metrics_loaded_success_populates_snapshot_and_daily_cost() {
        let mut app = test_app();
        handle_backend_metrics_loaded(&mut app, Ok(sample_tokens()), Ok(sample_costs(1.23)));
        let backend = app.layout.metrics.backend.as_ref().expect("snapshot set");
        assert!(backend.tokens.is_ok());
        assert!(backend.costs.is_ok());
        assert_eq!(app.dashboard.daily_cost_cents, 123);
    }

    #[test]
    fn handle_backend_metrics_loaded_partial_failure_preserves_the_other_half() {
        let mut app = test_app();
        handle_backend_metrics_loaded(
            &mut app,
            Ok(sample_tokens()),
            Err("costs endpoint unreachable".to_string()),
        );
        let backend = app.layout.metrics.backend.as_ref().expect("snapshot set");
        assert!(
            backend.tokens.is_ok(),
            "tokens half must survive a costs failure"
        );
        assert!(backend.costs.is_err());
        // WHY: a failed costs fetch must not silently reset the display to
        // zero cents — it should leave the last-known value alone.
        assert_eq!(app.dashboard.daily_cost_cents, 0);
    }

    #[test]
    fn handle_backend_metrics_loaded_tokens_failure_preserves_costs() {
        let mut app = test_app();
        handle_backend_metrics_loaded(
            &mut app,
            Err("tokens endpoint unreachable".to_string()),
            Ok(sample_costs(4.56)),
        );
        let backend = app.layout.metrics.backend.as_ref().expect("snapshot set");
        assert!(backend.tokens.is_err());
        assert!(
            backend.costs.is_ok(),
            "costs half must survive a tokens failure"
        );
        assert_eq!(app.dashboard.daily_cost_cents, 456);
    }

    #[test]
    fn backend_is_stale_false_before_first_fetch() {
        let app = test_app();
        assert!(!app.layout.metrics.backend_is_stale());
    }
}
