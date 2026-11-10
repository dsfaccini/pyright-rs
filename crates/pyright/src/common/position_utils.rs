//! Port of `packages/pyright-internal/src/common/positionUtils.ts`.

use super::char_codes::Char;
use super::text_range::{Position, Range, TextRange};
use super::text_range_collection::TextRangeCollection;
use crate::parser::tokenizer::TokenizerOutput;

// Translates a file offset into a line/column pair.
// common/positionUtils.ts:17
pub fn convert_offset_to_position(offset: i32, lines: &TextRangeCollection<TextRange>) -> Position {
    // Handle the case where the file is empty.
    if lines.end() == 0 {
        return Position {
            line: 0,
            character: 0,
        };
    }

    let item_index = if offset >= lines.end() {
        lines.count() - 1
    } else {
        lines.get_item_containing(offset)
    };
    assert!(item_index >= 0 && item_index <= lines.count());
    let line_range = lines.get_item_at(item_index);
    Position {
        line: item_index,
        character: 0.max(line_range.length.min(offset - line_range.start)),
    }
}

// Translates a start/end file offset into a pair of line/column positions.
// common/positionUtils.ts:37
pub fn convert_offsets_to_range(
    start_offset: i32,
    end_offset: i32,
    lines: &TextRangeCollection<TextRange>,
) -> Range {
    let start = convert_offset_to_position(start_offset, lines);

    // Fast path: when the end offset falls on the same line as the start, derive
    // the end position directly from the start's line range.
    if lines.end() != 0 && end_offset >= start_offset {
        let line_range = lines.get_item_at(start.line);
        let line_end = line_range.start + line_range.length;
        if end_offset >= line_range.start && end_offset < line_end {
            return Range {
                start,
                end: Position {
                    line: start.line,
                    character: end_offset - line_range.start,
                },
            };
        }
    }

    let end = convert_offset_to_position(end_offset, lines);
    Range { start, end }
}

// Translates a position (line and col) into a file offset.
// common/positionUtils.ts:69
pub fn convert_position_to_offset(
    position: &Position,
    lines: &TextRangeCollection<TextRange>,
) -> Option<i32> {
    if position.line >= lines.count() {
        return None;
    }

    Some(lines.get_item_at(position.line).start + position.character)
}

// common/positionUtils.ts:77
pub fn convert_range_to_text_range(
    range: &Range,
    lines: &TextRangeCollection<TextRange>,
) -> Option<TextRange> {
    let start = convert_position_to_offset(&range.start, lines)?;
    let end = convert_position_to_offset(&range.end, lines)?;
    Some(TextRange::from_bounds(start, end))
}

// common/positionUtils.ts:91
pub fn convert_text_range_to_range(
    range: &TextRange,
    lines: &TextRangeCollection<TextRange>,
) -> Range {
    convert_offsets_to_range(range.start, TextRange::get_end(range), lines)
}

// Returns the position of the last character in a line (before the newline).
// common/positionUtils.ts:96
pub fn get_line_end_position(
    tokenizer_output: &TokenizerOutput,
    text: &[u16],
    line: i32,
) -> Position {
    convert_offset_to_position(
        get_line_end_offset(tokenizer_output, text, line),
        &tokenizer_output.lines,
    )
}

// common/positionUtils.ts:100
pub fn get_line_end_offset(tokenizer_output: &TokenizerOutput, text: &[u16], line: i32) -> i32 {
    let line_range = tokenizer_output.lines.get_item_at(line);

    let line_end_offset = TextRange::get_end(&line_range);
    let mut new_line_length = 0;
    let mut i = line_end_offset - 1;
    while i >= line_range.start {
        // `text[i]` past the end is `undefined`, which is neither newline character.
        let char = text.get(i as usize).copied();
        if char != Some(Char::CarriageReturn) && char != Some(Char::LineFeed) {
            break;
        }

        new_line_length += 1;
        i -= 1;
    }

    // Character should be at the end of the line but before the newline.
    line_end_offset - new_line_length
}
