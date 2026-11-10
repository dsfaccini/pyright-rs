pub mod base_uri;
pub mod file_uri;
pub mod memoization;
#[allow(clippy::module_inception)]
pub mod uri;
pub mod uri_utils;

pub use uri::Uri;
