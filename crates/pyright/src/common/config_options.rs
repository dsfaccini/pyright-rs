//! Port of `packages/pyright-internal/src/common/configOptions.ts`.

// common/configOptions.ts:89
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticLevel {
    None,
    Information,
    Warning,
    Error,
}

impl DiagnosticLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticLevel::None => "none",
            DiagnosticLevel::Information => "information",
            DiagnosticLevel::Warning => "warning",
            DiagnosticLevel::Error => "error",
        }
    }
}
