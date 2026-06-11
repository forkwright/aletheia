use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum PrinterError {
    #[snafu(display("Chromium binary not found; install chromium or set CHROMIUM_PATH env var"))]
    ChromiumNotFound,
    #[snafu(display("Browser launch failed: {source}"))]
    BrowserLaunch {
        #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, |e| e)))]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("Page error: {source}"))]
    PageError {
        #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, |e| e)))]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("PDF generation failed: {source}"))]
    PdfError {
        #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, |e| e)))]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("{operation} timed out after {timeout_secs}s"))]
    Timeout {
        operation: &'static str,
        timeout_secs: u64,
    },
    #[snafu(display("Browser cleanup failed: {source}"))]
    Cleanup {
        #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, |e| e)))]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}
