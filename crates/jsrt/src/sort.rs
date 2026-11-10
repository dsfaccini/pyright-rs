//! V8's `Array.prototype.sort`: the TimSort in V8's
//! `third_party/v8/builtins/array-sort.tq`, V8 14.6 (Node 26).
//!
//! It makes the same comparator calls, in the same order, with the same
//! arguments as V8. A comparator that is not a total order, or that has side
//! effects, therefore gives the same permutation and the same effects as in
//! pyright. Sorting a permutation of indices keeps it independent of `T`.

use std::cmp::Ordering;

use crate::string::JsStr;

/// The result of a JS comparator: only its sign matters, and NaN counts as 0.
pub trait SortOrder {
    fn order(self) -> Ordering;
}

impl SortOrder for f64 {
    fn order(self) -> Ordering {
        // array-sort.tq SortCompareUserFn: "If v is NaN, return +0."
        if self < 0.0 {
            Ordering::Less
        } else if self > 0.0 {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }
}

impl SortOrder for i32 {
    fn order(self) -> Ordering {
        self.cmp(&0)
    }
}

impl SortOrder for i64 {
    fn order(self) -> Ordering {
        self.cmp(&0)
    }
}

impl SortOrder for Ordering {
    fn order(self) -> Ordering {
        self
    }
}

/// JS `array.sort(compare)`, with V8's comparator call sequence.
///
/// JS moves `undefined` and holes to the end without calling the comparator;
/// a Rust slice holds neither.
pub fn sort_by<T, R: SortOrder>(v: &mut [T], compare: impl FnMut(&T, &T) -> R) {
    let n = v.len();
    if n < 2 {
        return;
    }
    let mut state = SortState {
        v: &*v,
        compare,
        work: (0..n).collect(),
        temp: Vec::new(),
        pending_runs: Vec::new(),
        min_gallop: MIN_GALLOP_WINS,
    };
    // array-sort.tq ArrayTimSort.
    if n < 8 {
        state.binary_insertion_sort(0, 0, n);
    } else {
        state.tim_sort(n);
    }
    let order = state.work;
    apply_permutation(v, &order);
}

/// JS `array.sort()` with no comparator, on strings: UTF-16 code unit order.
pub fn sort_strings<S: AsRef<str>>(v: &mut [S]) {
    sort_by(v, |a, b| a.as_ref().js_cmp(b.as_ref()));
}

/// Moves `v[order[i]]` to `v[i]`, following each cycle of the permutation.
fn apply_permutation<T>(v: &mut [T], order: &[usize]) {
    let mut done = vec![false; order.len()];
    for start in 0..order.len() {
        let mut i = start;
        while !done[i] {
            done[i] = true;
            let next = order[i];
            if next == start {
                break;
            }
            v.swap(i, next);
            i = next;
        }
    }
}

const MIN_GALLOP_WINS: usize = 7;

#[derive(Clone, Copy)]
enum Arr {
    Work,
    Temp,
}

enum MergeExit {
    Succeed,
    CopyRest,
}

fn ix(i: isize) -> usize {
    usize::try_from(i).expect("merge cursor in bounds")
}

fn signed(n: usize) -> isize {
    isize::try_from(n).expect("array length fits isize")
}

/// array-sort.tq SortState. `work` and `temp` hold indices into `v`.
struct SortState<'a, T, F> {
    v: &'a [T],
    compare: F,
    work: Vec<usize>,
    temp: Vec<usize>,
    pending_runs: Vec<(usize, usize)>,
    min_gallop: usize,
}

impl<T, R: SortOrder, F: FnMut(&T, &T) -> R> SortState<'_, T, F> {
    fn compare(&mut self, x: usize, y: usize) -> Ordering {
        (self.compare)(&self.v[x], &self.v[y]).order()
    }

    fn at(&self, arr: Arr, i: usize) -> usize {
        match arr {
            Arr::Work => self.work[i],
            Arr::Temp => self.temp[i],
        }
    }

    // array-sort.tq BinaryInsertionSort
    fn binary_insertion_sort(&mut self, low: usize, start_arg: usize, high: usize) {
        let mut start = if low == start_arg {
            start_arg + 1
        } else {
            start_arg
        };
        while start < high {
            let mut left = low;
            let mut right = start;
            let pivot = self.work[right];
            while left < right {
                let mid = left + ((right - left) >> 1);
                if self.compare(pivot, self.work[mid]) == Ordering::Less {
                    right = mid;
                } else {
                    left = mid + 1;
                }
            }
            self.work.copy_within(left..start, left + 1);
            self.work[left] = pivot;
            start += 1;
        }
    }

    // array-sort.tq CountAndMakeRun
    fn count_and_make_run(&mut self, low_arg: usize, high: usize) -> usize {
        let low = low_arg + 1;
        if low == high {
            return 1;
        }
        let mut run_length = 2;
        let element_low = self.work[low];
        let element_low_pred = self.work[low - 1];
        let is_descending = self.compare(element_low, element_low_pred) == Ordering::Less;
        let mut previous_element = element_low;
        for idx in low + 1..high {
            let current_element = self.work[idx];
            let order = self.compare(current_element, previous_element);
            if is_descending {
                if order != Ordering::Less {
                    break;
                }
            } else if order == Ordering::Less {
                break;
            }
            previous_element = current_element;
            run_length += 1;
        }
        if is_descending {
            self.work[low_arg..low_arg + run_length].reverse();
        }
        run_length
    }

    // array-sort.tq MergeAt
    fn merge_at(&mut self, i: usize) {
        let stack_size = self.pending_runs.len();
        let (mut base_a, mut length_a) = self.pending_runs[i];
        let (base_b, mut length_b) = self.pending_runs[i + 1];
        self.pending_runs[i].1 = length_a + length_b;
        if i + 3 == stack_size {
            self.pending_runs[i + 1] = self.pending_runs[i + 2];
        }
        self.pending_runs.pop();

        let key_right = self.work[base_b];
        let k = self.gallop_right(Arr::Work, key_right, base_a, length_a, 0);
        base_a += k;
        length_a -= k;
        if length_a == 0 {
            return;
        }
        let key_left = self.work[base_a + length_a - 1];
        length_b = self.gallop_left(Arr::Work, key_left, base_b, length_b, length_b - 1);
        if length_b == 0 {
            return;
        }
        if length_a <= length_b {
            self.merge_low(base_a, length_a, base_b, length_b);
        } else {
            self.merge_high(base_a, length_a, base_b, length_b);
        }
    }

    // array-sort.tq GallopLeft. Returns the offset in `0..=length` where `key`
    // belongs, left of any equal elements.
    fn gallop_left(
        &mut self,
        arr: Arr,
        key: usize,
        base: usize,
        length: usize,
        hint: usize,
    ) -> usize {
        let mut last_ofs = 0;
        let mut offset = 1;
        let (mut lo, mut hi);
        if self.compare(self.at(arr, base + hint), key) == Ordering::Less {
            let max_ofs = length - hint;
            while offset < max_ofs {
                if self.compare(self.at(arr, base + hint + offset), key) != Ordering::Less {
                    break;
                }
                last_ofs = offset;
                offset = (offset << 1) + 1;
            }
            offset = offset.min(max_ofs);
            lo = last_ofs + hint + 1;
            hi = offset + hint;
        } else {
            let max_ofs = hint + 1;
            while offset < max_ofs {
                if self.compare(self.at(arr, base + hint - offset), key) == Ordering::Less {
                    break;
                }
                last_ofs = offset;
                offset = (offset << 1) + 1;
            }
            offset = offset.min(max_ofs);
            lo = hint + 1 - offset;
            hi = hint - last_ofs;
        }
        while lo < hi {
            let m = lo + ((hi - lo) >> 1);
            if self.compare(self.at(arr, base + m), key) == Ordering::Less {
                lo = m + 1;
            } else {
                hi = m;
            }
        }
        hi
    }

    // array-sort.tq GallopRight. Returns the offset in `0..=length` where
    // `key` belongs, right of any equal elements.
    fn gallop_right(
        &mut self,
        arr: Arr,
        key: usize,
        base: usize,
        length: usize,
        hint: usize,
    ) -> usize {
        let mut last_ofs = 0;
        let mut offset = 1;
        let (mut lo, mut hi);
        if self.compare(key, self.at(arr, base + hint)) == Ordering::Less {
            let max_ofs = hint + 1;
            while offset < max_ofs {
                if self.compare(key, self.at(arr, base + hint - offset)) != Ordering::Less {
                    break;
                }
                last_ofs = offset;
                offset = (offset << 1) + 1;
            }
            offset = offset.min(max_ofs);
            lo = hint + 1 - offset;
            hi = hint - last_ofs;
        } else {
            let max_ofs = length - hint;
            while offset < max_ofs {
                if self.compare(key, self.at(arr, base + hint + offset)) == Ordering::Less {
                    break;
                }
                last_ofs = offset;
                offset = (offset << 1) + 1;
            }
            offset = offset.min(max_ofs);
            lo = last_ofs + hint + 1;
            hi = offset + hint;
        }
        while lo < hi {
            let m = lo + ((hi - lo) >> 1);
            if self.compare(key, self.at(arr, base + m)) == Ordering::Less {
                hi = m;
            } else {
                lo = m + 1;
            }
        }
        hi
    }

    // array-sort.tq MergeLow
    fn merge_low(&mut self, base_a: usize, length_a: usize, base_b: usize, length_b: usize) {
        let (mut length_a, mut length_b) = (length_a, length_b);
        self.temp.clear();
        self.temp
            .extend_from_slice(&self.work[base_a..base_a + length_a]);
        let mut dest = base_a;
        let mut cursor_temp = 0;
        let mut cursor_b = base_b;
        self.work[dest] = self.work[cursor_b];
        dest += 1;
        cursor_b += 1;

        let exit = 'merge: {
            length_b -= 1;
            if length_b == 0 {
                break 'merge MergeExit::Succeed;
            }
            if length_a == 1 {
                break 'merge MergeExit::CopyRest;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let mut nof_wins_a = 0;
                let mut nof_wins_b = 0;
                loop {
                    if self.compare(self.work[cursor_b], self.temp[cursor_temp]) == Ordering::Less {
                        self.work[dest] = self.work[cursor_b];
                        dest += 1;
                        cursor_b += 1;
                        nof_wins_b += 1;
                        length_b -= 1;
                        nof_wins_a = 0;
                        if length_b == 0 {
                            break 'merge MergeExit::Succeed;
                        }
                        if nof_wins_b >= min_gallop {
                            break;
                        }
                    } else {
                        self.work[dest] = self.temp[cursor_temp];
                        dest += 1;
                        cursor_temp += 1;
                        nof_wins_a += 1;
                        length_a -= 1;
                        nof_wins_b = 0;
                        if length_a == 1 {
                            break 'merge MergeExit::CopyRest;
                        }
                        if nof_wins_a >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first_iteration = true;
                while nof_wins_a >= MIN_GALLOP_WINS
                    || nof_wins_b >= MIN_GALLOP_WINS
                    || first_iteration
                {
                    first_iteration = false;
                    min_gallop = 1.max(min_gallop - 1);
                    self.min_gallop = min_gallop;
                    nof_wins_a =
                        self.gallop_right(Arr::Temp, self.work[cursor_b], cursor_temp, length_a, 0);
                    if nof_wins_a > 0 {
                        self.work[dest..dest + nof_wins_a]
                            .copy_from_slice(&self.temp[cursor_temp..cursor_temp + nof_wins_a]);
                        dest += nof_wins_a;
                        cursor_temp += nof_wins_a;
                        length_a -= nof_wins_a;
                        if length_a == 1 {
                            break 'merge MergeExit::CopyRest;
                        }
                        if length_a == 0 {
                            break 'merge MergeExit::Succeed;
                        }
                    }
                    self.work[dest] = self.work[cursor_b];
                    dest += 1;
                    cursor_b += 1;
                    length_b -= 1;
                    if length_b == 0 {
                        break 'merge MergeExit::Succeed;
                    }
                    nof_wins_b =
                        self.gallop_left(Arr::Work, self.temp[cursor_temp], cursor_b, length_b, 0);
                    if nof_wins_b > 0 {
                        self.work.copy_within(cursor_b..cursor_b + nof_wins_b, dest);
                        dest += nof_wins_b;
                        cursor_b += nof_wins_b;
                        length_b -= nof_wins_b;
                        if length_b == 0 {
                            break 'merge MergeExit::Succeed;
                        }
                    }
                    self.work[dest] = self.temp[cursor_temp];
                    dest += 1;
                    cursor_temp += 1;
                    length_a -= 1;
                    if length_a == 1 {
                        break 'merge MergeExit::CopyRest;
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        };
        match exit {
            MergeExit::Succeed => {
                if length_a > 0 {
                    self.work[dest..dest + length_a]
                        .copy_from_slice(&self.temp[cursor_temp..cursor_temp + length_a]);
                }
            }
            MergeExit::CopyRest => {
                self.work.copy_within(cursor_b..cursor_b + length_b, dest);
                self.work[dest + length_b] = self.temp[cursor_temp];
            }
        }
    }

    // array-sort.tq MergeHigh. Cursors walk down and may end at -1.
    fn merge_high(&mut self, base_a: usize, length_a: usize, base_b: usize, length_b: usize) {
        let (mut length_a, mut length_b) = (length_a, length_b);
        self.temp.clear();
        self.temp
            .extend_from_slice(&self.work[base_b..base_b + length_b]);
        let mut dest = signed(base_b + length_b - 1);
        let mut cursor_temp = signed(length_b - 1);
        let mut cursor_a = signed(base_a + length_a - 1);
        self.work[ix(dest)] = self.work[ix(cursor_a)];
        dest -= 1;
        cursor_a -= 1;

        let exit = 'merge: {
            length_a -= 1;
            if length_a == 0 {
                break 'merge MergeExit::Succeed;
            }
            if length_b == 1 {
                break 'merge MergeExit::CopyRest;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let mut nof_wins_a = 0;
                let mut nof_wins_b = 0;
                loop {
                    if self.compare(self.temp[ix(cursor_temp)], self.work[ix(cursor_a)])
                        == Ordering::Less
                    {
                        self.work[ix(dest)] = self.work[ix(cursor_a)];
                        dest -= 1;
                        cursor_a -= 1;
                        nof_wins_a += 1;
                        length_a -= 1;
                        nof_wins_b = 0;
                        if length_a == 0 {
                            break 'merge MergeExit::Succeed;
                        }
                        if nof_wins_a >= min_gallop {
                            break;
                        }
                    } else {
                        self.work[ix(dest)] = self.temp[ix(cursor_temp)];
                        dest -= 1;
                        cursor_temp -= 1;
                        nof_wins_b += 1;
                        length_b -= 1;
                        nof_wins_a = 0;
                        if length_b == 1 {
                            break 'merge MergeExit::CopyRest;
                        }
                        if nof_wins_b >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first_iteration = true;
                while nof_wins_a >= MIN_GALLOP_WINS
                    || nof_wins_b >= MIN_GALLOP_WINS
                    || first_iteration
                {
                    first_iteration = false;
                    min_gallop = 1.max(min_gallop - 1);
                    self.min_gallop = min_gallop;
                    let k = self.gallop_right(
                        Arr::Work,
                        self.temp[ix(cursor_temp)],
                        base_a,
                        length_a,
                        length_a - 1,
                    );
                    nof_wins_a = length_a - k;
                    if nof_wins_a > 0 {
                        dest -= signed(nof_wins_a);
                        cursor_a -= signed(nof_wins_a);
                        let from = ix(cursor_a + 1);
                        self.work.copy_within(from..from + nof_wins_a, ix(dest + 1));
                        length_a -= nof_wins_a;
                        if length_a == 0 {
                            break 'merge MergeExit::Succeed;
                        }
                    }
                    self.work[ix(dest)] = self.temp[ix(cursor_temp)];
                    dest -= 1;
                    cursor_temp -= 1;
                    length_b -= 1;
                    if length_b == 1 {
                        break 'merge MergeExit::CopyRest;
                    }
                    let k = self.gallop_left(
                        Arr::Temp,
                        self.work[ix(cursor_a)],
                        0,
                        length_b,
                        length_b - 1,
                    );
                    nof_wins_b = length_b - k;
                    if nof_wins_b > 0 {
                        dest -= signed(nof_wins_b);
                        cursor_temp -= signed(nof_wins_b);
                        let from = ix(cursor_temp + 1);
                        let to = ix(dest + 1);
                        self.work[to..to + nof_wins_b]
                            .copy_from_slice(&self.temp[from..from + nof_wins_b]);
                        length_b -= nof_wins_b;
                        if length_b == 1 {
                            break 'merge MergeExit::CopyRest;
                        }
                        if length_b == 0 {
                            break 'merge MergeExit::Succeed;
                        }
                    }
                    self.work[ix(dest)] = self.work[ix(cursor_a)];
                    dest -= 1;
                    cursor_a -= 1;
                    length_a -= 1;
                    if length_a == 0 {
                        break 'merge MergeExit::Succeed;
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        };
        match exit {
            MergeExit::Succeed => {
                if length_b > 0 {
                    let to = ix(dest - (signed(length_b) - 1));
                    self.work[to..to + length_b].copy_from_slice(&self.temp[..length_b]);
                }
            }
            MergeExit::CopyRest => {
                dest -= signed(length_a);
                cursor_a -= signed(length_a);
                let from = ix(cursor_a + 1);
                self.work.copy_within(from..from + length_a, ix(dest + 1));
                self.work[ix(dest)] = self.temp[ix(cursor_temp)];
            }
        }
    }

    // array-sort.tq RunInvariantEstablished
    fn run_invariant_established(&self, n: usize) -> bool {
        if n < 2 {
            return true;
        }
        let runs = &self.pending_runs;
        runs[n - 2].1 > runs[n - 1].1 + runs[n].1
    }

    // array-sort.tq MergeCollapse
    fn merge_collapse(&mut self) {
        while self.pending_runs.len() > 1 {
            let mut n = self.pending_runs.len() - 2;
            if !self.run_invariant_established(n + 1) || !self.run_invariant_established(n) {
                if self.pending_runs[n - 1].1 < self.pending_runs[n + 1].1 {
                    n -= 1;
                }
                self.merge_at(n);
            } else if self.pending_runs[n].1 <= self.pending_runs[n + 1].1 {
                self.merge_at(n);
            } else {
                break;
            }
        }
    }

    // array-sort.tq MergeForceCollapse
    fn merge_force_collapse(&mut self) {
        while self.pending_runs.len() > 1 {
            let mut n = self.pending_runs.len() - 2;
            if n > 0 && self.pending_runs[n - 1].1 < self.pending_runs[n + 1].1 {
                n -= 1;
            }
            self.merge_at(n);
        }
    }

    // array-sort.tq ArrayTimSortImpl
    fn tim_sort(&mut self, length: usize) {
        let mut remaining = length;
        let mut low = 0;
        let min_run_length = compute_min_run_length(remaining);
        while remaining != 0 {
            let mut current_run_length = self.count_and_make_run(low, low + remaining);
            if current_run_length < min_run_length {
                let forced_run_length = min_run_length.min(remaining);
                self.binary_insertion_sort(low, low + current_run_length, low + forced_run_length);
                current_run_length = forced_run_length;
            }
            self.pending_runs.push((low, current_run_length));
            self.merge_collapse();
            low += current_run_length;
            remaining -= current_run_length;
        }
        self.merge_force_collapse();
    }
}

// array-sort.tq ComputeMinRunLength
fn compute_min_run_length(n_arg: usize) -> usize {
    let mut n = n_arg;
    let mut r = 0;
    while n >= 64 {
        r |= n & 1;
        n >>= 1;
    }
    n + r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_and_keeps_equal_elements_in_order() {
        let mut v: Vec<(i32, i32)> = (0..300).map(|i| ((i * 7919) % 13, i)).collect();
        sort_by(&mut v, |a, b| a.0 - b.0);
        assert!(
            v.windows(2)
                .all(|w| w[0].0 < w[1].0 || (w[0].0 == w[1].0 && w[0].1 < w[1].1))
        );
    }

    #[test]
    fn default_sort_is_utf16_order() {
        let mut v = vec!["\u{FF5E}", "\u{1F600}", "b", "a"];
        sort_strings(&mut v);
        assert_eq!(v, ["a", "b", "\u{1F600}", "\u{FF5E}"]);
    }
}
