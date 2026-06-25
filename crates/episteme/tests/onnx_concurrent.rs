#![cfg(all(feature = "gliner", feature = "nuextract"))]

use std::path::Path;

use episteme::bookkeeping::{BookkeepingResult, GlinerExtractionProvider, NuExtractProvider};
use episteme::extract::{ExtractionConfig, ExtractionEngine, ExtractionError, ExtractionProvider};

struct NeverFallback;

impl ExtractionProvider for NeverFallback {
    fn complete<'a>(
        &'a self,
        _system: &'a str,
        _user_message: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<String, ExtractionError>> + Send + 'a>,
    > {
        Box::pin(async move { unreachable!("concurrent ONNX test should not call LLM fallback") })
    }
}

#[tokio::test]
async fn concurrent_onnx_extraction_keeps_tokio_responsive() -> BookkeepingResult<()> {
    let gliner_dir = Path::new("/models/onnx/gliner_multi-v2.1");
    let nuextract_dir = Path::new("/models/onnx/nuextract-2b");
    if !gliner_dir.exists() || !nuextract_dir.exists() {
        return Ok(());
    }

    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let fallback = NeverFallback;
    let gliner = GlinerExtractionProvider::new(&engine, &fallback)?;
    let nuextract = NuExtractProvider::new()?;

    // WHY: If ONNX inference still occupied tokio worker threads, the two
    // concurrent smoke_infer futures would stall the runtime and this join
    // would not return promptly. spawn_blocking lets the runtime stay alive.
    let (gliner_result, nuextract_result) =
        tokio::join!(gliner.smoke_infer(), nuextract.smoke_infer());

    gliner_result?;
    nuextract_result?;
    Ok(())
}
