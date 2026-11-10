//! JS `Array.prototype` methods whose index rules differ from Rust's.

use crate::string::slice_range;

/// JS `array.slice(start, end)`: a negative index counts from the end, and
/// both indexes clamp to the array, as for `String.prototype.slice`. Pass
/// `None` for an omitted `end`. JS returns a new array; this borrows the same
/// elements, so call `to_vec()` for the copy.
pub fn slice<T>(v: &[T], start: i32, end: impl Into<Option<i32>>) -> &[T] {
    let len = i32::try_from(v.len()).expect("a JS array length fits in i32");
    let (from, to) = slice_range(start, end.into(), len);
    &v[from..to]
}
