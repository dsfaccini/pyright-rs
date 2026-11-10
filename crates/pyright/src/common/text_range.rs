//! Port of `packages/pyright-internal/src/common/textRange.ts`.

use std::cmp::Ordering;

use jsrt::P;

/// What TypeScript gets from structural typing: tokens, comments and parse
/// nodes all "are" a `TextRange`. Functions that take a `TextRange` in
/// TypeScript take any `HasTextRange` here.
pub trait HasTextRange {
    fn text_range(&self) -> TextRange;
}

impl<T: HasTextRange> HasTextRange for P<T> {
    fn text_range(&self) -> TextRange {
        (**self).text_range()
    }
}

impl<T: HasTextRange + ?Sized> HasTextRange for &T {
    fn text_range(&self) -> TextRange {
        (**self).text_range()
    }
}

// common/textRange.ts:12
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[derive(jsrt::Trace)]
pub struct TextRange {
    pub start: i32,
    pub length: i32,
}

impl HasTextRange for TextRange {
    fn text_range(&self) -> TextRange {
        *self
    }
}

impl TextRange {
    // common/textRange.ts:18
    pub fn create(start: i32, length: i32) -> TextRange {
        if start < 0 {
            panic!("start must be non-negative");
        }
        if length < 0 {
            panic!("length must be non-negative");
        }
        TextRange { start, length }
    }

    // common/textRange.ts:28
    pub fn from_bounds(start: i32, end: i32) -> TextRange {
        if start < 0 {
            panic!("start must be non-negative");
        }
        if start > end {
            panic!("end must be greater than or equal to start");
        }
        TextRange::create(start, end - start)
    }

    // common/textRange.ts:38
    pub fn get_end(range: &(impl HasTextRange + ?Sized)) -> i32 {
        let range = range.text_range();
        range.start + range.length
    }

    // common/textRange.ts:42
    pub fn contains(range: &(impl HasTextRange + ?Sized), position: i32) -> bool {
        let r = range.text_range();
        position >= r.start && position < TextRange::get_end(&r)
    }

    // common/textRange.ts:46
    pub fn contains_range(
        range: &(impl HasTextRange + ?Sized),
        span: &(impl HasTextRange + ?Sized),
    ) -> bool {
        let range = range.text_range();
        let span = span.text_range();
        span.start >= range.start && TextRange::get_end(&span) <= TextRange::get_end(&range)
    }

    // common/textRange.ts:50
    pub fn overlaps(range: &(impl HasTextRange + ?Sized), position: i32) -> bool {
        let r = range.text_range();
        position >= r.start && position <= TextRange::get_end(&r)
    }

    // common/textRange.ts:54
    pub fn overlaps_range(
        range: &(impl HasTextRange + ?Sized),
        other: &(impl HasTextRange + ?Sized),
    ) -> bool {
        let range = range.text_range();
        let other = other.text_range();
        TextRange::overlaps(&range, other.start) || TextRange::overlaps(&other, range.start)
    }

    // common/textRange.ts:58
    pub fn extend(
        range: &(impl HasTextRange + ?Sized),
        extension: &(impl HasTextRange + ?Sized),
    ) -> TextRange {
        let mut result = range.text_range();
        let extension = extension.text_range();

        if extension.start < result.start {
            result = TextRange {
                start: extension.start,
                length: result.length + result.start - extension.start,
            };
        }

        let extension_end = TextRange::get_end(&extension);
        let result_end = TextRange::get_end(&result);
        if extension_end > result_end {
            result = TextRange {
                start: result.start,
                length: result.length + extension_end - result_end,
            };
        }

        result
    }

    // common/textRange.ts:80
    pub fn combine<R: HasTextRange>(ranges: &[R]) -> Option<TextRange> {
        if ranges.is_empty() {
            return None;
        }

        let first = ranges[0].text_range();
        let mut combined_range = TextRange {
            start: first.start,
            length: first.length,
        };
        for range in &ranges[1..] {
            combined_range = TextRange::extend(&combined_range, range);
        }
        Some(combined_range)
    }
}

// common/textRange.ts:93
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[derive(jsrt::Trace)]
pub struct Position {
    // Both line and column are zero-based
    pub line: i32,
    pub character: i32,
}

impl Position {
    // common/textRange.ts:100
    pub fn print(value: &Position) -> String {
        format!("({}:{})", value.line, value.character)
    }
}

// common/textRange.ts:105
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[derive(jsrt::Trace)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    // common/textRange.ts:111
    pub fn print(value: &Range) -> String {
        format!(
            "{}-{}",
            Position::print(&value.start),
            Position::print(&value.end)
        )
    }
}

// common/textRange.ts:117
pub fn compare_positions(a: &Position, b: &Position) -> i32 {
    if a.line < b.line {
        -1
    } else if a.line > b.line {
        1
    } else if a.character < b.character {
        -1
    } else if a.character > b.character {
        1
    } else {
        0
    }
}

/// `compare_positions` as an `Ordering`, for Rust sort calls.
pub fn compare_positions_ordering(a: &Position, b: &Position) -> Ordering {
    compare_positions(a, b).cmp(&0)
}

// common/textRange.ts:130
pub fn get_empty_position() -> Position {
    Position {
        line: 0,
        character: 0,
    }
}

// common/textRange.ts:137
pub fn do_ranges_overlap(a: &Range, b: &Range) -> bool {
    if compare_positions(&b.start, &a.end) >= 0 {
        return false;
    } else if compare_positions(&a.start, &b.end) >= 0 {
        return false;
    }
    true
}

// common/textRange.ts:146
pub fn do_ranges_intersect(a: &Range, b: &Range) -> bool {
    if compare_positions(&b.start, &a.end) > 0 {
        return false;
    } else if compare_positions(&a.start, &b.end) > 0 {
        return false;
    }
    true
}

// common/textRange.ts:155
pub fn is_position_in_range(range: &Range, position: &Position) -> bool {
    compare_positions(&range.start, position) <= 0 && compare_positions(&range.end, position) >= 0
}

// common/textRange.ts:159
pub fn is_range_in_range(range: &Range, contained_range: &Range) -> bool {
    is_position_in_range(range, &contained_range.start)
        && is_position_in_range(range, &contained_range.end)
}

// common/textRange.ts:163
pub fn positions_are_equal(a: &Position, b: &Position) -> bool {
    compare_positions(a, b) == 0
}

// common/textRange.ts:167
pub fn ranges_are_equal(a: &Range, b: &Range) -> bool {
    positions_are_equal(&a.start, &b.start) && positions_are_equal(&a.end, &b.end)
}

// common/textRange.ts:171
pub fn get_empty_range() -> Range {
    Range {
        start: get_empty_position(),
        end: get_empty_position(),
    }
}

// common/textRange.ts:178
pub fn is_empty_position(pos: &Position) -> bool {
    pos.character == 0 && pos.line == 0
}

// common/textRange.ts:182
pub fn is_empty_range(range: &Range) -> bool {
    is_empty_position(&range.start) && is_empty_position(&range.end)
}

// common/textRange.ts:186
pub fn extend_range(range: &mut Range, extension: &Range) {
    if compare_positions(&extension.start, &range.start) < 0 {
        range.start = extension.start;
    }

    if compare_positions(&extension.end, &range.end) > 0 {
        range.end = extension.end;
    }
}

/// Mutates and returns `ranges[0]`, as the TypeScript does.
// common/textRange.ts:196
pub fn combine_range(ranges: &mut [Range]) -> Option<Range> {
    if ranges.is_empty() {
        return None;
    }

    let (first, rest) = ranges.split_at_mut(1);
    let combined_range = &mut first[0];
    for range in rest.iter() {
        extend_range(combined_range, range);
    }

    Some(*combined_range)
}
