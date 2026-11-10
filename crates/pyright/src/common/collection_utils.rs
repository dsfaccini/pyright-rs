//! Port of `packages/pyright-internal/src/common/collectionUtils.ts`.
//!
//! Helpers whose Rust equivalent is a one-line iterator call (`map`, `some`,
//! `every`, `find`, `flatten`, `getNestedProperty`) are not ported; callers use
//! the iterator directly, in the same order.

use std::hash::Hash;

use jsrt::JsMap;

use crate::common::core::Comparison;

// common/collectionUtils.ts:16
pub fn contains<T>(
    array: Option<&[T]>,
    value: &T,
    equality_comparer: impl Fn(&T, &T) -> bool,
) -> bool {
    if let Some(array) = array {
        for v in array {
            if equality_comparer(v, value) {
                return true;
            }
        }
    }
    false
}

// common/collectionUtils.ts:46
pub fn append<T>(to: Option<Vec<T>>, value: Option<T>) -> Option<Vec<T>> {
    let Some(value) = value else {
        return to;
    };
    match to {
        None => Some(vec![value]),
        Some(mut to) => {
            to.push(value);
            Some(to)
        }
    }
}

// common/collectionUtils.ts:61
pub fn append_array<T: Clone>(to: &mut Vec<T>, elements_to_push: &[T]) {
    to.extend_from_slice(elements_to_push);
}

// common/collectionUtils.ts:72
pub fn partition<T>(
    array: impl IntoIterator<Item = T>,
    cb: impl Fn(&T) -> bool,
) -> (Vec<T>, Vec<T>) {
    let mut true_items = Vec::new();
    let mut false_items = Vec::new();

    for item in array {
        if cb(&item) {
            true_items.push(item);
        } else {
            false_items.push(item);
        }
    }

    (true_items, false_items)
}

// common/collectionUtils.ts:105
fn to_offset(len: usize, offset: isize) -> usize {
    if offset < 0 {
        (len as isize + offset) as usize
    } else {
        offset as usize
    }
}

// common/collectionUtils.ts:126
/// `addRange` for a defined `to`. `from` elements are never `undefined` in Rust.
pub fn add_range<T: Clone>(
    to: &mut Vec<T>,
    from: Option<&[T]>,
    start: Option<isize>,
    end: Option<isize>,
) {
    let Some(from) = from else {
        return;
    };
    if from.is_empty() {
        return;
    }
    let start = start.map_or(0, |s| to_offset(from.len(), s));
    let end = end.map_or(from.len(), |e| to_offset(from.len(), e));
    let mut i = start;
    while i < end && i < from.len() {
        to.push(from[i].clone());
        i += 1;
    }
}

// common/collectionUtils.ts:149
pub fn insert_at<T>(array: &mut Vec<T>, index: usize, value: T) {
    array.insert(index, value);
}

// common/collectionUtils.ts:180
/// `array.slice().sort(comparer)`. V8 sorts with a stable TimSort; for a
/// consistent comparer every stable sort gives the same order.
pub fn clone_and_sort<T: Clone>(array: &[T], comparer: impl Fn(&T, &T) -> Comparison) -> Vec<T> {
    let mut result = array.to_vec();
    result.sort_by(|a, b| comparer(a, b).to_ordering());
    result
}

// common/collectionUtils.ts:200
pub fn stable_sort<T: Clone>(array: &[T], comparer: impl Fn(&T, &T) -> Comparison) -> Vec<T> {
    let mut indices: Vec<usize> = (0..array.len()).collect();
    indices.sort_by(|&x, &y| match comparer(&array[x], &array[y]) {
        Comparison::EqualTo => x.cmp(&y),
        c => c.to_ordering(),
    });
    indices.into_iter().map(|i| array[i].clone()).collect()
}

// common/collectionUtils.ts:257
pub fn binary_search<T, U>(
    array: &[T],
    value: &T,
    key_selector: impl Fn(&T) -> U,
    key_comparer: impl Fn(&U, &U) -> Comparison,
    offset: Option<usize>,
) -> isize {
    binary_search_key(
        array,
        &key_selector(value),
        key_selector,
        key_comparer,
        offset,
    )
}

// common/collectionUtils.ts:279
pub fn binary_search_key<T, U>(
    array: &[T],
    key: &U,
    key_selector: impl Fn(&T) -> U,
    key_comparer: impl Fn(&U, &U) -> Comparison,
    offset: Option<usize>,
) -> isize {
    if array.is_empty() {
        return -1;
    }

    let mut low = offset.unwrap_or(0) as isize;
    let mut high = array.len() as isize - 1;
    while low <= high {
        let middle = low + ((high - low) >> 1);
        let mid_key = key_selector(&array[middle as usize]);
        match key_comparer(&mid_key, key) {
            Comparison::LessThan => low = middle + 1,
            Comparison::EqualTo => return middle,
            Comparison::GreaterThan => high = middle - 1,
        }
    }

    !low
}

// common/collectionUtils.ts:330
pub fn get_or_add<K: Hash + Eq, V: Clone>(
    map: &mut JsMap<K, V>,
    key: K,
    new_value_factory: impl FnOnce() -> V,
) -> V {
    if let Some(value) = map.get(&key) {
        return value.clone();
    }

    let new_value = new_value_factory();
    map.insert(key, new_value.clone());

    new_value
}

// common/collectionUtils.ts:346
pub fn remove_array_elements<T>(array: &mut Vec<T>, predicate: impl Fn(&T) -> bool) {
    array.retain(|item| !predicate(item));
}

// common/collectionUtils.ts:360
pub fn create_map_from_items<T>(
    items: impl IntoIterator<Item = T>,
    key_getter: impl Fn(&T) -> String,
) -> JsMap<String, Vec<T>> {
    let mut map: JsMap<String, Vec<T>> = JsMap::default();
    for item in items {
        let key = key_getter(&item);
        map.entry(key).or_default().push(item);
    }
    map
}

// common/collectionUtils.ts:374
pub fn add_if_unique<T>(arr: &mut Vec<T>, t: T, equality_comparer: impl Fn(&T, &T) -> bool) {
    if contains(Some(arr), &t, equality_comparer) {
        return;
    }

    arr.push(t);
}

// common/collectionUtils.ts:383
pub fn get_map_values<K, V: Clone>(m: &JsMap<K, V>, predicate: impl Fn(&K, &V) -> bool) -> Vec<V> {
    m.iter()
        .filter(|(k, v)| predicate(k, v))
        .map(|(_, v)| v.clone())
        .collect()
}

// common/collectionUtils.ts:403
pub fn array_equals<T>(c1: &[T], c2: &[T], predicate: impl Fn(&T, &T) -> bool) -> bool {
    if c1.len() != c2.len() {
        return false;
    }

    c1.iter().zip(c2).all(|(a, b)| predicate(a, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::core::compare_values;

    // Ported from tests/collectionUtils.test.ts.

    #[test]
    fn utils_append_undefined() {
        assert_eq!(append::<i32>(None, None), None);
    }

    #[test]
    fn utils_append_undefined_to_empty() {
        assert_eq!(append::<i32>(Some(vec![]), None), Some(vec![]));
    }

    #[test]
    fn utils_append_to_undefined() {
        assert_eq!(append(None, Some(1)), Some(vec![1]));
    }

    #[test]
    fn utils_append_to_empty() {
        assert_eq!(append(Some(vec![]), Some(1)), Some(vec![1]));
    }

    #[test]
    fn utils_append_to_non_empty() {
        assert_eq!(append(Some(vec![1]), Some(2)), Some(vec![1, 2]));
    }

    #[test]
    fn utils_add_range_to_undefined() {
        let mut to: Vec<i32> = vec![];
        add_range(&mut to, Some(&[1, 2]), None, None);
        assert_eq!(to, vec![1, 2]);
    }

    #[test]
    fn utils_add_range_offsets() {
        let mut to = vec![0];
        add_range(&mut to, Some(&[1, 2, 3, 4]), Some(1), Some(-1));
        assert_eq!(to, vec![0, 2, 3]);
    }

    #[test]
    fn utils_insert_at() {
        let mut data = vec![2, 3, 4];
        insert_at(&mut data, 0, 1);
        insert_at(&mut data, 4, 5);
        insert_at(&mut data, 2, 9);
        assert_eq!(data, vec![1, 2, 9, 3, 4, 5]);
    }

    #[test]
    fn utils_clone_and_sort() {
        let data = [3, 2, 1];
        let sorted = clone_and_sort(&data, |a, b| {
            compare_values(Some(*a as f64), Some(*b as f64))
        });
        assert_eq!(sorted, vec![1, 2, 3]);
        assert_eq!(data, [3, 2, 1]);
    }

    #[test]
    fn utils_stable_sort() {
        let data = [(2, 'a'), (1, 'b'), (2, 'c'), (1, 'd')];
        let sorted = stable_sort(&data, |a, b| {
            compare_values(Some(a.0 as f64), Some(b.0 as f64))
        });
        assert_eq!(sorted, vec![(1, 'b'), (1, 'd'), (2, 'a'), (2, 'c')]);
    }

    #[test]
    fn utils_binary_search() {
        let data = [1, 2, 3, 4, 5];
        let cmp = |a: &i32, b: &i32| compare_values(Some(*a as f64), Some(*b as f64));
        assert_eq!(binary_search(&data, &3, |v| *v, cmp, None), 2);
        assert_eq!(binary_search(&data, &6, |v| *v, cmp, None), !5);
        assert_eq!(binary_search(&[] as &[i32], &1, |v| *v, cmp, None), -1);
    }

    #[test]
    fn utils_partition() {
        let (evens, odds) = partition([1, 2, 3, 4], |v| v % 2 == 0);
        assert_eq!(evens, vec![2, 4]);
        assert_eq!(odds, vec![1, 3]);
    }

    #[test]
    fn utils_create_map_from_items() {
        let map = create_map_from_items(["b1", "a1", "b2"], |s| s[..1].to_string());
        let keys: Vec<&String> = map.keys().collect();
        assert_eq!(keys, ["b", "a"]);
        assert_eq!(map["b"], vec!["b1", "b2"]);
    }
}
