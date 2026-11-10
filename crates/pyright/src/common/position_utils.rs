//! Port of `packages/pyright-internal/src/common/positionUtils.ts`.
//!
//! SHIM from the infra branch: only what `diagnostic_sink.rs` needs. The
//! frontend branch owns this file; its port replaces this one wholesale.

use crate::common::text_range::{Position, Range, TextRange};
use crate::common::text_range_collection::TextRangeCollection;

// common/positionUtils.ts:17
pub fn convert_offset_to_position(offset: i32, lines: &TextRangeCollection<TextRange>) -> Position {
    if lines.end() == 0 {
        return Position { line: 0, character: 0 };
    }

    let item_index = if offset >= lines.end() { lines.count() - 1 } else { lines.get_item_containing(offset) };
    let line_range = lines.get_item_at(item_index);
    Position { line: item_index, character: 0.max(line_range.length.min(offset - line_range.start)) }
}

// common/positionUtils.ts:37
pub fn convert_offsets_to_range(start_offset: i32, end_offset: i32, lines: &TextRangeCollection<TextRange>) -> Range {
    let start = convert_offset_to_position(start_offset, lines);
    let end = convert_offset_to_position(end_offset, lines);
    Range { start, end }
}
