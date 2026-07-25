//! wc-core — domain types, config path helpers, formats, backend routing, errors.

pub mod backend_routing;
pub mod behavior_setting;
pub mod config;
pub mod config_normalizer;
pub mod error;
pub mod formats;
pub mod types;

pub use config::ConfigDir;
pub use error::WcError;
pub use types::{FileType, WallpaperEntry};

#[cfg(test)]
mod tests;
