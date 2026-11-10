//! Port of `packages/pyright-internal/src/common/textRangeCollection.ts`.
//!
//! SHIM from the infra branch: only what `diagnostic_sink.rs` needs. The
//! frontend branch owns this file; its port replaces this one wholesale.

use crate::common::text_range::TextRange;

// common/textRangeCollection.ts:17
pub struct TextRangeCollection<T> {
    items: Vec<T>,
}

impl TextRangeCollection<TextRange> {
    pub fn new(items: Vec<TextRange>) -> Self {
        TextRangeCollection { items }
    }

    pub fn end(&self) -> i32 {
        self.items.last().map_or(0, |last| last.start + last.length)
    }

    pub fn count(&self) -> i32 {
        self.items.len() as i32
    }

    pub fn get_item_at(&self, index: i32) -> TextRange {
        self.items[index as usize]
    }

    pub fn get_item_containing(&self, position: i32) -> i32 {
        let start = self.items.first().map_or(0, |first| first.start);
        if self.items.is_empty() || position < start || position > self.end() {
            return -1;
        }
        self.items
            .iter()
            .position(|item| position >= item.start && position < item.start + item.length)
            .map_or(-1, |i| i as i32)
    }
}
