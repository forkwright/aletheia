# L3 API Index: basanos

Crate path: `crates/basanos`

Public API signatures extracted from source. Doc comments shown above each signature.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum Error {
    /// Failed to read a file.
    #[snafu(display("failed to read file {}", path.display()))]
    ReadFile {
        /// Path to the file.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Failed to scan directory.
    #[snafu(display("failed to read directory {}", path.display()))]
    ReadDir {
        /// Path to the directory.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },
}
```

> Shorthand for fallible operations.
```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## `src/rules/mod.rs`

```rust
pub struct Violation {
    /// Rule identifier, e.g. `PLANNING/missing-falsifier`.
    pub rule: String,
    /// File path where the violation was found.
    pub path: String,
    /// Approximate line number (1-based).
    pub line: usize,
    /// Human-readable message.
    pub message: String,
}
```

> A lint rule that can be applied to a project tree.
```rust
pub trait Rule {
    fn id (&self) -> &'static str;
    fn check (&self, project_root: &str) -> Result<Vec<Violation>>;
}
```

> All registered rules.
```rust
pub fn all_rules () -> Vec<Box<dyn Rule>>
```

## `src/rules/planning.rs`

> Rule: PLANNING/missing-falsifier.
> 
> Ensures every phase PLAN.md has a Falsification section that covers
> all success criteria, and that vision.md / ROADMAP.md do not contain
> unfalsifiable adjectives without measurement.
```rust
pub struct MissingFalsifierRule;
```
