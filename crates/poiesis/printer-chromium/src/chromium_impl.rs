use std::path::PathBuf;

use chromiumoxide::{Browser, BrowserConfig};
use futures::StreamExt as _;
use tracing::debug;

use crate::{PrintOptions, PrinterError};

pub(crate) async fn print_to_pdf_inner(
    html: &str,
    opts: &PrintOptions,
) -> Result<Vec<u8>, PrinterError> {
    let exe = find_chromium().ok_or(PrinterError::ChromiumNotFound)?;
    debug!(path = %exe.display(), "launching Chromium");

    let config = BrowserConfig::builder()
        .chrome_executable(exe)
        .arg("--no-sandbox")
        .arg("--disable-gpu")
        .arg("--disable-dev-shm-usage")
        .build()
        .map_err(|e| PrinterError::BrowserLaunch {
            source: Box::from(e),
        })?;

    let (mut browser, mut handler) =
        Browser::launch(config)
            .await
            .map_err(|e| PrinterError::BrowserLaunch {
                source: Box::from(e),
            })?;

    let _handle = tokio::spawn(async move { while let Some(_ev) = handler.next().await {} });

    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| PrinterError::PageError {
            source: Box::from(e),
        })?;

    page.set_content(html)
        .await
        .map_err(|e| PrinterError::PageError {
            source: Box::from(e),
        })?;

    // Convert mm to inches for CDP (1 in = 25.4 mm)
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

    let pdf_bytes = page
        .pdf(pdf_params)
        .await
        .map_err(|e| PrinterError::PdfError {
            source: Box::from(e),
        })?;

    browser.close().await.ok();

    Ok(pdf_bytes)
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
