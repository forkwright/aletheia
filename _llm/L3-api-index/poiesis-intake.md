# L3 API Index: poiesis-intake

Crate path: `crates/poiesis/intake`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/lib.rs`

```rust
pub enum RequestKind {
    /// Research or analytical task.
    Analysis,
    /// Written report or narrative document.
    Report,
    /// Dashboard or visual panel.
    Dashboard,
    /// Could not be classified.
    Unclassified,
}
```

```rust
pub struct IntakeRequest {
    /// Classified kind of the request.
    pub kind: RequestKind,
    /// URL-safe slug derived from the description.
    pub slug: String,
    /// Normalised description text.
    pub description: String,
    /// Extracted requirement bullets (empty if none found).
    pub requirements: Vec<String>,
}
```

```rust
pub enum Error {
    /// The intake text could not be parsed.
    #[snafu(display("intake parse failed: {message}"))]
    ParseIntake {
        /// Human-readable reason.
        message: String,
    },
}
```

> Convenience alias.
```rust
pub type Result<T> = std::result::Result<T, Error>;
```

> Parse free-form intake text into a structured [`IntakeRequest`].
>
> Classification is keyword-based and case-insensitive.  The first matching
> category wins in the order: Analysis, Report, Dashboard.  If no keyword
> matches the request is [`RequestKind::Unclassified`].
>
> # Errors
>
> Returns [`Error::ParseIntake`] when the input is empty or cannot be
> normalised.
```rust
pub fn parse_intake (text: &str) -> Result<IntakeRequest>
```

> Generate a skeleton file list for the given intake request.
>
> # Errors
>
> Currently infallible, but returns [`Result`] for forward compatibility.
```rust
pub fn generate_scaffold (req: &IntakeRequest) -> Result<Vec<ScaffoldFile>>
```
