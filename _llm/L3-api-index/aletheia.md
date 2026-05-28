# L3 API Index: aletheia

Crate path: `crates/aletheia`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub struct Error {
    message: String,
    // kanon:ignore RUST/box-dyn-error — snafu Whatever pattern: generic error source for ergonomic wrapping in binary crate
    #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, Some)))]
    // kanon:ignore RUST/box-dyn-error — snafu Whatever pattern: generic error source for ergonomic wrapping in binary crate
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}
```
