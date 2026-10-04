//! Shared skill bundles and wire contracts for the gateway and its CLI.

pub mod api;
mod bundle;
mod frontmatter;
mod manifest;
mod zip_validation;

pub use api::*;
pub use bundle::{
    BundleError, BundleLimits, INSTALL_RECORD, MAX_INSTRUCTIONS_BYTES, ValidatedBundle,
    inspect_archive, is_reserved_install_path, pack_directory, pack_directory_excluding,
};
pub use manifest::{validate_file_path, validate_name, validate_namespace};
