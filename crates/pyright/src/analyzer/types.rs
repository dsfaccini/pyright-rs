//! Placeholder for `packages/pyright-internal/src/analyzer/types.ts`, which
//! the evaluator step ports. Holds only what the binder group names.

/// A pyright type. The binder never creates one; `Symbol` names it for
/// synthesized symbols.
pub struct Type {
    _private: (),
}

// analyzer/types.ts:85
pub type TypeVarScopeId = String;
