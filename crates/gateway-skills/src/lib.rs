//! Shared skill bundles and wire contracts for the gateway and its CLI.

pub mod api;
mod bundle;
mod frontmatter;
mod manifest;
mod zip_validation;

pub use api::*;
pub use bundle::{
    BundleError, BundleLimits, ValidatedBundle, inspect_archive, pack_directory,
    pack_directory_excluding,
};
pub use manifest::{validate_file_path, validate_name, validate_namespace};
