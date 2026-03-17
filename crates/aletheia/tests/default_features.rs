//! Guards against accidental removal of required default features.
//! Skipped by feature-isolation CI jobs (--no-default-features).

#[test]
#[cfg(all(feature = "tui", feature = "recall", feature = "storage-fjall"))]
fn embed_candle_is_in_default_features() {
    #[expect(
        clippy::assertions_on_constants,
        reason = "intentional compile-time feature guard"
    )]
    {
        assert!(
            cfg!(feature = "embed-candle"),
            "embed-candle must be in default features (see #1263, #1326, #1378)"
        );
    }
}
