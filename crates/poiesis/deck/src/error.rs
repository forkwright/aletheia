use snafu::Snafu;

/// Errors that can occur during deck rendering.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum DeckError {
    /// Component not found in registry.
    #[snafu(display("component '{id}' not found in registry"))]
    ComponentNotFound {
        /// The missing component id.
        id: String,
    },

    /// Failed to load template file.
    #[snafu(display("failed to load template for '{id}': {source}"))]
    TemplateLoad {
        /// The component id.
        id: String,
        /// The underlying IO error.
        source: std::io::Error,
    },

    /// Template render error.
    #[snafu(display("template render error for '{id}': {source}"))]
    TemplateRender {
        /// The component id.
        id: String,
        /// The underlying minijinja error.
        source: minijinja::Error,
    },
}
