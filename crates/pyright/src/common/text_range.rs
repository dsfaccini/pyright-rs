//! Port of `packages/pyright-internal/src/common/textRange.ts`.
//!
//! SHIM from the infra branch: only the types `diagnostic.rs` needs. The
//! frontend branch owns this file; its port replaces this one wholesale.

// common/textRange.ts:12
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextRange {
    pub start: i32,
    pub length: i32,
}

// common/textRange.ts:93
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    pub line: i32,
    pub character: i32,
}

// common/textRange.ts:105
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

// common/textRange.ts:178
pub fn is_empty_position(pos: Position) -> bool {
    pos.character == 0 && pos.line == 0
}

// common/textRange.ts:182
pub fn is_empty_range(range: Range) -> bool {
    is_empty_position(range.start) && is_empty_position(range.end)
}
