//! Port of pyright 1.1.414 `packages/pyright-internal/src`. See `docs/porting.md`.

pub mod analyzer;
pub mod common;
pub mod file_system_mapping;
pub mod localization;
pub mod oracle;
pub mod parser;
pub mod partial_stub_service;
pub mod pyright;
pub mod pyright_file_system;
pub mod readonly_augmented_file_system;
#[cfg(test)]
pub mod tests;
