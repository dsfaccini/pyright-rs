//! Port of `packages/pyright-internal/src/common/caseSensitivityDetector.ts`.

// common/caseSensitivityDetector.ts:9
pub trait CaseSensitivityDetector {
    fn is_case_sensitive(&self, uri: &str) -> bool;
}

/// `UriEx`'s detector that answers `true` for every Uri.
pub struct CaseSensitiveDetector;

impl CaseSensitivityDetector for CaseSensitiveDetector {
    fn is_case_sensitive(&self, _uri: &str) -> bool {
        true
    }
}
