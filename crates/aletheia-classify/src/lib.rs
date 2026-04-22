#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Author-classifier inference handler for training data decontamination.
//!
//! This crate provides the aletheia-side runtime for filtering AI-generated
//! text from the training-data capture pipeline. It loads an ONNX model and
//! metadata sidecar produced by gnomon research, and exposes a simple
//! classification interface for use in the `nous::training::capture` gate.
//!
//! See the design doc at `forkwright/aletheia#3786` for the full specification
//! and artifact contract.
//!
//! # Artifact contract
//!
//! The classifier expects two files in a directory:
//!
//! - `model.onnx` — ONNX binary model (typically TF-IDF + logistic regression)
//! - `metadata.json` — artifact metadata with schema version, producer, and evaluation results
//!
//! The default artifact location is `/data/models/author-classifier/v1/`, but this
//! is configurable at daemon startup.
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

/// ONNX-based author classification and inference.
pub mod classifier;
/// Error types for author classifier operations.
pub mod error;
