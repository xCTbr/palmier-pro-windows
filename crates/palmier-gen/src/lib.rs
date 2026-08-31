//! Image generation behind a provider trait.
//!
//! Constitution: generation is optional everywhere, providers are bring-your-own-key, and
//! nothing in the project model knows a provider exists. This crate is the only place
//! that does.

pub mod keys;
pub mod stitch;

pub use keys::{KeyError, KeyRing};
pub use stitch::Stitch;

/// An image a provider produced, in memory.
#[derive(Clone)]
pub struct GeneratedImage {
    pub bytes: Vec<u8>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    /// File extension for the bytes, without a dot.
    pub extension: &'static str,
}

impl std::fmt::Debug for GeneratedImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeneratedImage")
            .field("bytes", &self.bytes.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GenError {
    #[error("no API keys configured for {provider} — add one before generating")]
    NoKeys { provider: &'static str },
    #[error("could not reach the provider: {0}")]
    Transport(String),
    #[error("{0}")]
    Provider(String),
}
