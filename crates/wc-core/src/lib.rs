//! wc-core — domain types, config resolution, formats, backend routing, errors.

pub mod backend_routing;
pub mod config;
pub mod config_normalizer;
pub mod error;
pub mod formats;
pub mod types;

pub use config::ConfigDir;
pub use error::WcError;
// FormatRegistry not yet defined — reserved for future extension
mod tests;
pub use types::{FileType, WallpaperEntry};
