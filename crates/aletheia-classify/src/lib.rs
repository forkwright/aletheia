#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Author-classifier inference handler for training data decontamination.
//!
//! This crate provides the aletheia-side runtime for filtering AI-generated
//! text from the training-data capture pipeline. It uses a lightweight
//! heuristic rule bank (surface-feature scoring) that requires no external
//! model artifacts, and exposes a simple classification interface for use in
//! the `nous::training::capture` gate.
//!
//! See the design doc at `forkwright/aletheia#3786` for the full specification
//! and artifact contract.
//!
//! # Artifact contract
//!
//! The optional `Classifier::load` path expects `metadata.json` in a directory
//! for observability and version tracking, but no model file is required.
//! The heuristic engine is fully embedded.
//!
//! # Example
//!
//! ```ignore
//! use aletheia_classify::{Classifier, AuthorClass};
//! use std::path::Path;
//!
//! let classifier = Classifier::load(Path::new("/data/models/author-classifier/v1/"))
//!     .await?;
//!
//! let probs = classifier.classify("Hello, world!")?;
//! match probs.argmax() {
//!     AuthorClass::User => println!("User text"),
//!     AuthorClass::Subagent => println!("AI-generated"),
//!     _ => println!("System or template"),
//! }
//! ```

pub use classifier::{AuthorClass, AuthorProbs, Classifier};
pub use error::{ClassifyError, Result};

/// Heuristic rule-bank author classification and inference.
pub mod classifier;
/// Error types for author classifier operations.
pub mod error;
