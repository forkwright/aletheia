use std::future::Future;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chromiumoxide::{Browser, BrowserConfig};
use futures::StreamExt as _;
use tracing::{debug, error, warn, Instrument as _};

use crate::{PrintOptions, PrinterError};

pub(crate) async fn print_to_pdf_inner(
    html: &str,
    opts: &PrintOptions,
) -> Result<Vec<u8>, PrinterError> {
    let exe = find_chromium().ok_or(PrinterError::ChromiumNotFound)?;
    debug!(path = %exe.display(), "launching Chromium");

    let mut config = BrowserConfig::builder()
        .chrome_executable(exe)
        .arg("--disable-gpu")
        .arg("--disable-dev-shm-usage");

    if opts.disable_sandbox || disable_sandbox_from_env() {
        // WHY: Some locked-down containers lack user namespaces; sandbox disablement is explicit because it weakens Chromium's process isolation.
        warn!("launching Chromium with sandbox disabled");
        config = config.arg("--no-sandbox");
    }

    let config = config.build().map_err(|e| PrinterError::BrowserLaunch {
        source: Box::from(e),
    })?;

    let started = Instant::now();
    let (mut browser, mut handler) = with_remaining_timeout(
        started,
        opts.timeout,
        "browser launch",
        Browser::launch(config),
    )
    .await?
    .map_err(|e| PrinterError::BrowserLaunch {
        source: Box::from(e),
    })?;

    let handler_task = tokio::spawn(
        async move { while let Some(_ev) = handler.next().await {} }
            .instrument(tracing::Span::current()),
    );

    let render_result = render_with_browser(&mut browser, html, opts, started).await;
    let cleanup_result = cleanup_browser(&mut browser, Duration::from_secs(5)).await;
    handler_task.abort();

    match (render_result, cleanup_result) {
        (Ok(pdf_bytes), Ok(())) => Ok(pdf_bytes),
        (Ok(pdf_bytes), Err(cleanup_error)) => {
            warn!(error = %cleanup_error, "Chromium cleanup failed; returning successful PDF");
            Ok(pdf_bytes)
        }
        (Err(render_error), Ok(())) => Err(render_error),
        (Err(render_error), Err(cleanup_error)) => {
            error!(error = %cleanup_error, "Chromium cleanup failed after render error");
            Err(render_error)
        }
    }
}

async fn render_with_browser(
    browser: &mut Browser,
    html: &str,
    opts: &PrintOptions,
    started: Instant,
) -> Result<Vec<u8>, PrinterError> {
    let page = with_remaining_timeout(
        started,
        opts.timeout,
        "page creation",
        browser.new_page("about:blank"),
    )
    .await?
    .map_err(|e| PrinterError::PageError {
        source: Box::from(e),
    })?;

    with_remaining_timeout(
        started,
        opts.timeout,
        "page content",
        page.set_content(html),
    )
    .await?
    .map_err(|e| PrinterError::PageError {
        source: Box::from(e),
    })?;

    // WHY: CDP PrintToPdf takes inches; layout options are mm (1 in = 25.4 mm).
    let mm_to_in = |mm: f64| mm / 25.4;

    let pdf_params = chromiumoxide::cdp::browser_protocol::page::PrintToPdfParams::builder()
        .paper_width(mm_to_in(opts.paper_width_mm))
        .paper_height(mm_to_in(opts.paper_height_mm))
        .margin_top(mm_to_in(opts.margin_top_mm))
        .margin_bottom(mm_to_in(opts.margin_bottom_mm))
        .margin_left(mm_to_in(opts.margin_left_mm))
        .margin_right(mm_to_in(opts.margin_right_mm))
        .scale(opts.scale)
        .print_background(true)
        .build();

    let pdf_bytes = with_remaining_timeout(
        started,
        opts.timeout,
        "pdf generation",
        page.pdf(pdf_params),
    )
    .await?
    .map_err(|e| PrinterError::PdfError {
        source: Box::from(e),
    })?;

    Ok(pdf_bytes)
}

async fn with_remaining_timeout<T, F>(
    started: Instant,
    timeout: Duration,
    operation: &'static str,
    future: F,
) -> Result<T, PrinterError>
where
    F: Future<Output = T>,
{
    let remaining = timeout
        .checked_sub(started.elapsed())
        .ok_or(PrinterError::Timeout {
            operation,
            timeout_secs: timeout.as_secs().max(1),
        })?;

    tokio::time::timeout(remaining, future)
        .await
        .map_err(|_elapsed| PrinterError::Timeout {
            operation,
            timeout_secs: timeout.as_secs().max(1),
        })
}

async fn cleanup_browser(browser: &mut Browser, timeout: Duration) -> Result<(), PrinterError> {
    match tokio::time::timeout(timeout, browser.close()).await {
        Ok(Ok(_closed)) => {}
        Ok(Err(err)) => {
            error!(error = %err, "Chromium close failed");
            kill_browser(browser).await;
            return Err(PrinterError::Cleanup {
                source: Box::from(err),
            });
        }
        Err(_elapsed) => {
            warn!("Chromium close timed out; killing browser process");
            kill_browser(browser).await;
            return Err(PrinterError::Timeout {
                operation: "browser close",
                timeout_secs: timeout.as_secs().max(1),
            });
        }
    }

    match tokio::time::timeout(timeout, browser.wait()).await {
        Ok(Ok(_status)) => Ok(()),
        Ok(Err(err)) => {
            error!(error = %err, "Chromium wait after close failed");
            kill_browser(browser).await;
            Err(PrinterError::Cleanup {
                source: Box::from(err),
            })
        }
        Err(_elapsed) => {
            warn!("Chromium wait after close timed out; killing browser process");
            kill_browser(browser).await;
            Err(PrinterError::Timeout {
                operation: "browser wait",
                timeout_secs: timeout.as_secs().max(1),
            })
        }
    }
}

async fn kill_browser(browser: &mut Browser) {
    if let Some(Err(err)) = browser.kill().await {
        error!(error = %err, "Chromium kill failed");
    }
}

fn disable_sandbox_from_env() -> bool {
    matches!(
        std::env::var("POIESIS_CHROMIUM_DISABLE_SANDBOX").as_deref(),
        Ok("1" | "true" | "TRUE" | "yes" | "YES")
    )
}

fn find_chromium() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("CHROMIUM_PATH") {
        let pb = PathBuf::from(&path);
        if pb.exists() {
            return Some(pb);
        }
    }
    for candidate in &[
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
    ] {
        if let Ok(path) = which::which(candidate) {
            return Some(path);
        }
    }
    None
}
