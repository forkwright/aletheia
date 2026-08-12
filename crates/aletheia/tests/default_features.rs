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

// WARNING: storage-fjall must remain in defaults. Every store that holds
// operator memory must default to the durable engine, never a silently
// transient one (see #4661). Gated on the other default features so the
// guard itself keeps compiling if storage-fjall is ever dropped.
#[test]
#[cfg(all(
    feature = "tui",
    feature = "recall",
    feature = "embed-candle",
    feature = "cc-provider"
))]
fn storage_fjall_is_in_default_features() {
    #[expect(
        clippy::assertions_on_constants,
        reason = "intentional compile-time feature guard"
    )]
    {
        assert!(
            cfg!(feature = "storage-fjall"),
            "storage-fjall must be in default features (see #4661)"
        );
    }
}
