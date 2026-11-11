//! Port of `packages/pyright-internal/src/common/textRangeCollection.ts`.

use std::cell::Cell;

use super::text_range::{HasTextRange, TextRange};

// common/textRangeCollection.ts:17
#[derive(jsrt::Trace)]
pub struct TextRangeCollection<T: HasTextRange + Copy> {
    _items: Vec<T>,

    // Index most recently returned by getItemContaining. Only a hint that is
    // re-validated on each call, so it never changes the returned result.
    _last_hit_index: Cell<i32>,
}

impl<T: HasTextRange + Copy> TextRangeCollection<T> {
    // common/textRangeCollection.ts:27
    pub fn new(items: Vec<T>) -> Self {
        TextRangeCollection {
            _items: items,
            _last_hit_index: Cell::new(0),
        }
    }

    // common/textRangeCollection.ts:31
    pub fn start(&self) -> i32 {
        if !self._items.is_empty() {
            self._items[0].text_range().start
        } else {
            0
        }
    }

    // common/textRangeCollection.ts:35
    pub fn end(&self) -> i32 {
        if !self._items.is_empty() {
            let last_item = self._items[self._items.len() - 1].text_range();
            last_item.start + last_item.length
        } else {
            0
        }
    }

    // common/textRangeCollection.ts:40
    pub fn length(&self) -> i32 {
        self.end() - self.start()
    }

    // common/textRangeCollection.ts:44
    pub fn count(&self) -> i32 {
        self._items.len() as i32
    }

    // common/textRangeCollection.ts:48
    pub fn contains(&self, position: i32) -> bool {
        position >= self.start() && position < self.end()
    }

    // common/textRangeCollection.ts:52
    pub fn get_item_at(&self, index: i32) -> T {
        if index < 0 || index >= self._items.len() as i32 {
            panic!("index is out of range");
        }
        self._items[index as usize]
    }

    /// The items, for iteration in order.
    pub fn items(&self) -> &[T] {
        &self._items
    }

    // Returns the nearest item prior to the position.
    // The position may not be contained within the item.
    // common/textRangeCollection.ts:61
    pub fn get_item_at_position(&self, position: i32) -> i32 {
        if self.count() == 0 {
            return -1;
        }
        if position < self.start() {
            return -1;
        }
        if position > self.end() {
            return -1;
        }

        let mut min = 0;
        let mut max = self.count() - 1;

        while min < max {
            let mid = min + ((max - min) >> 1);
            let item = self._items[mid as usize].text_range();

            // Is the position past the start of this item but before
            // the start of the next item? If so, we found our item.
            if position >= item.start {
                if mid >= self.count() - 1
                    || position < self._items[(mid + 1) as usize].text_range().start
                {
                    return mid;
                }
            }

            if position < item.start {
                max = mid - 1;
            } else {
                min = mid + 1;
            }
        }
        min
    }

    // common/textRangeCollection.ts:96
    pub fn get_item_containing(&self, position: i32) -> i32 {
        if self.count() == 0 {
            return -1;
        }
        if position < self.start() {
            return -1;
        }
        if position > self.end() {
            return -1;
        }

        // Fast path: check the last returned index and its forward neighbor before
        // falling back to the binary search.
        let last_hit = self._last_hit_index.get();
        if last_hit >= 0 && last_hit < self._items.len() as i32 {
            let item = self._items[last_hit as usize];
            if TextRange::contains(&item, position) {
                return last_hit;
            }

            let next_index = last_hit + 1;
            if next_index < self._items.len() as i32 {
                let next_item = self._items[next_index as usize];
                if TextRange::contains(&next_item, position) {
                    self._last_hit_index.set(next_index);
                    return next_index;
                }
            }
        }

        let items = &self._items;
        let index = get_index_containing_by(
            items.len(),
            |i| Some(&items[i]),
            position,
            |item: &T, position| TextRange::contains(item, position),
        );
        if index >= 0 {
            self._last_hit_index.set(index);
        }
        index
    }
}

// common/textRangeCollection.ts:138
pub fn get_index_containing<T: HasTextRange>(
    arr: &[Option<T>],
    position: i32,
    in_range: Option<&dyn Fn(&T, i32) -> bool>,
) -> i32 {
    let default_in_range = |item: &T, position: i32| TextRange::contains(item, position);
    let in_range: &dyn Fn(&T, i32) -> bool = in_range.unwrap_or(&default_in_range);
    get_index_containing_by(arr.len(), |i| arr[i].as_ref(), position, in_range)
}

/// `getIndexContaining` over any indexable sequence of possibly missing items,
/// so `TextRangeCollection` can search its `Vec<T>` without copying it.
fn get_index_containing_by<'a, T: HasTextRange + 'a>(
    len: usize,
    get: impl Fn(usize) -> Option<&'a T>,
    position: i32,
    in_range: impl Fn(&T, i32) -> bool,
) -> i32 {
    if len == 0 {
        return -1;
    }

    let mut min: i32 = 0;
    let mut max: i32 = len as i32 - 1;
    while min <= max {
        let mid = min + (max - min) / 2;
        let Some((element_index, element_item)) = find_non_null_element(&get, len, mid, min, max)
        else {
            return -1;
        };

        if in_range(element_item, position) {
            return element_index;
        }

        let Some((_, next_element_item)) = find_non_null_element(&get, len, mid + 1, mid + 1, max)
        else {
            return -1;
        };

        if mid < len as i32 - 1
            && TextRange::get_end(element_item) <= position
            && position < next_element_item.text_range().start
        {
            return -1;
        }

        if position < element_item.text_range().start {
            max = mid - 1;
        } else {
            min = mid + 1;
        }
    }

    -1
}

// common/textRangeCollection.ts:179
fn find_non_null_element<'a, T: 'a>(
    get: &impl Fn(usize) -> Option<&'a T>,
    len: usize,
    position: i32,
    min: i32,
    max: i32,
) -> Option<(i32, &'a T)> {
    // `arr[position]` past the end is `undefined` in JavaScript.
    let at = |i: i32| {
        if i >= 0 && (i as usize) < len {
            get(i as usize)
        } else {
            None
        }
    };

    if let Some(item) = at(position) {
        return Some((position, item));
    }

    // Search forward and backward until it finds non-null value.
    let mut i = position + 1;
    while i <= max {
        if let Some(item) = at(i) {
            return Some((i, item));
        }
        i += 1;
    }

    let mut i = position - 1;
    while i >= min {
        if let Some(item) = at(i) {
            return Some((i, item));
        }
        i -= 1;
    }

    None
}
