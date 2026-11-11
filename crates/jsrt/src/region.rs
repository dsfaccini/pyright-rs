//! `Region`: bump allocation into memory that is released all at once.
//!
//! A dropped region's chunks go to a per-thread pool for the next region. A
//! region made with `protect_on_drop` instead unmaps its pages' contents and
//! makes them inaccessible (`PROT_NONE`), never to be reused, so any later
//! access through a stale pointer faults.

use std::cell::{Cell, RefCell};

use crate::vm;

const CHUNK: usize = 256 << 10;
const POOL_MAX: usize = 64;

thread_local! {
    static POOL: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

pub struct Region {
    /// Each chunk's base and the end of its used bytes; the last chunk's use ends at `next`.
    chunks: RefCell<Vec<(usize, usize)>>,
    next: Cell<usize>,
    end: Cell<usize>,
    protect_on_drop: bool,
}

impl Region {
    pub fn new(protect_on_drop: bool) -> Region {
        Region { chunks: RefCell::new(Vec::new()), next: Cell::new(0), end: Cell::new(0), protect_on_drop }
    }

    /// Moves `value` into the region and returns its address. Its destructor
    /// never runs, and the memory lives until the region drops.
    #[inline]
    pub fn alloc<T>(&self, value: T) -> *mut T {
        const { assert!(size_of::<T>() <= CHUNK && align_of::<T>() <= 4096) };
        let mut at = self.next.get().next_multiple_of(align_of::<T>());
        if at + size_of::<T>() > self.end.get() {
            at = self.new_chunk();
        }
        self.next.set(at + size_of::<T>());
        let p = at as *mut T;
        // SAFETY: `at..at + size_of::<T>()` is unused, aligned memory of a chunk this region owns.
        unsafe { p.write(value) };
        p
    }

    #[cold]
    fn new_chunk(&self) -> usize {
        let mut chunks = self.chunks.borrow_mut();
        if let Some(last) = chunks.last_mut() {
            last.1 = self.next.get();
        }
        let base = POOL.with(|pool| pool.borrow_mut().pop()).unwrap_or_else(map_chunk);
        chunks.push((base, base));
        self.end.set(base + CHUNK);
        base
    }

    /// Calls `f(start, end)` on the used bytes of each chunk, in allocation order.
    pub fn for_each_used(&self, mut f: impl FnMut(usize, usize)) {
        let chunks = self.chunks.borrow();
        for (i, &(base, used)) in chunks.iter().enumerate() {
            f(base, if i + 1 == chunks.len() { self.next.get() } else { used });
        }
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        for &(base, _) in self.chunks.get_mut().iter() {
            if self.protect_on_drop {
                // SAFETY: the chunk is a mapping of `CHUNK` bytes this region owns,
                // and nothing may use its contents any more.
                unsafe { vm::retire(base as *mut u8, CHUNK) };
            } else if POOL.with(|pool| pool.borrow().len()) < POOL_MAX {
                POOL.with(|pool| pool.borrow_mut().push(base));
            } else {
                // SAFETY: as above; the mapping goes back to the kernel.
                unsafe { vm::unmap(base as *mut u8, CHUNK) };
            }
        }
    }
}

fn map_chunk() -> usize {
    let p = vm::map(CHUNK, true).unwrap_or_else(|| panic!("region: mmap of a {CHUNK}-byte chunk failed"));
    p as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_stay_in_place_across_chunks() {
        let region = Region::new(false);
        let ptrs: Vec<*mut [u64; 5]> = (0..20_000u64).map(|i| region.alloc([i; 5])).collect();
        for (i, p) in ptrs.iter().enumerate() {
            assert_eq!(unsafe { **p }, [i as u64; 5]);
        }
        let mut used = 0;
        region.for_each_used(|start, end| used += (end - start) / size_of::<[u64; 5]>());
        assert_eq!(used, 20_000);
    }

    #[test]
    fn dropped_chunks_are_reused() {
        let first = Region::new(false);
        let a = first.alloc(1u64) as usize;
        drop(first);
        let second = Region::new(false);
        assert_eq!(second.alloc(2u64) as usize, a);
    }

    #[test]
    fn protected_chunks_are_not_reused() {
        let first = Region::new(true);
        let a = first.alloc(1u64) as usize;
        drop(first);
        let second = Region::new(false);
        assert_ne!(second.alloc(2u64) as usize, a);
    }
}
