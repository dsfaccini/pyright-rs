//! `Region`: bump allocation into memory that is released all at once.
//!
//! A dropped region's chunks go to a process-wide pool for the next region, so a
//! region a parse worker fills and the main thread drops recycles its chunks
//! without unmapping them. A region made with `protect_on_drop` instead unmaps
//! its pages' contents and makes them inaccessible (`PROT_NONE`), never to be
//! reused, so any later access through a stale pointer faults.

use std::cell::{Cell, RefCell};
use std::ptr;
use std::sync::{Mutex, PoisonError};

use crate::vm;

const CHUNK: usize = 256 << 10;
const POOL_MAX: usize = 64;

/// A chunk no region holds.
struct Free(*mut u8);

// SAFETY: a pooled chunk belongs to no region and no thread until one takes it.
unsafe impl Send for Free {}

static POOL: Mutex<Vec<Free>> = Mutex::new(Vec::new());

fn pool() -> std::sync::MutexGuard<'static, Vec<Free>> {
    POOL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Every pointer into a chunk derives from the pointer `vm::map` returned for it,
/// which keeps the provenance of the whole chunk (`docs/porting.md`, "unsafe code").
pub struct Region {
    /// Each chunk's base and the end of its used bytes; the last chunk's use ends at `next`.
    chunks: RefCell<Vec<(*mut u8, *mut u8)>>,
    next: Cell<*mut u8>,
    end: Cell<*mut u8>,
    protect_on_drop: bool,
}

impl Region {
    pub fn new(protect_on_drop: bool) -> Region {
        let (next, end) = (Cell::new(ptr::null_mut()), Cell::new(ptr::null_mut()));
        Region { chunks: RefCell::new(Vec::new()), next, end, protect_on_drop }
    }

    /// Moves `value` into the region and returns its address. Its destructor
    /// never runs, and the memory lives until the region drops.
    #[inline]
    pub fn alloc<T>(&self, value: T) -> *mut T {
        const { assert!(size_of::<T>() <= CHUNK && align_of::<T>() <= 4096) };
        let mut at = self.next.get().map_addr(|addr| addr.next_multiple_of(align_of::<T>()));
        if at.addr() + size_of::<T>() > self.end.get().addr() {
            at = self.new_chunk();
        }
        self.next.set(at.wrapping_add(size_of::<T>()));
        let p = at.cast::<T>();
        // SAFETY: `at..at + size_of::<T>()` is unused, aligned memory of a chunk this region owns.
        unsafe { p.write(value) };
        p
    }

    #[cold]
    fn new_chunk(&self) -> *mut u8 {
        let mut chunks = self.chunks.borrow_mut();
        if let Some(last) = chunks.last_mut() {
            last.1 = self.next.get();
        }
        let pooled = pool().pop();
        let base = pooled.map_or_else(map_chunk, |free| free.0);
        chunks.push((base, base));
        self.end.set(base.wrapping_add(CHUNK));
        base
    }

    /// Calls `f(start, end)` on the used bytes of each chunk, in allocation order.
    pub fn for_each_used(&self, mut f: impl FnMut(*mut u8, *mut u8)) {
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
                unsafe { vm::retire(base, CHUNK) };
                continue;
            }
            let mut pool = pool();
            if pool.len() < POOL_MAX {
                pool.push(Free(base));
            } else {
                drop(pool);
                // SAFETY: as above; the mapping goes back to the kernel.
                unsafe { vm::unmap(base, CHUNK) };
            }
        }
    }
}

fn map_chunk() -> *mut u8 {
    vm::map(CHUNK, true).unwrap_or_else(|| panic!("region: mmap of a {CHUNK}-byte chunk failed"))
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
        region.for_each_used(|start, end| used += (end.addr() - start.addr()) / size_of::<[u64; 5]>());
        assert_eq!(used, 20_000);
    }

    /// Serializes the tests that read the process-wide pool.
    static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn dropped_chunks_are_reused() {
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        let first = Region::new(false);
        let a = first.alloc(1u64) as usize;
        drop(first);
        let second = Region::new(false);
        assert_eq!(second.alloc(2u64) as usize, a);
    }

    #[test]
    fn a_region_dropped_on_another_thread_recycles_its_chunk() {
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        struct Moved(Region);
        // SAFETY: the test moves the region to one other thread and touches it there only.
        unsafe impl Send for Moved {}
        let region = Moved(Region::new(false));
        let a = region.0.alloc(7u64);
        // SAFETY: `a` is a live value of the region.
        assert_eq!(unsafe { *a }, 7);
        let a = a as usize;
        std::thread::spawn(move || drop(region)).join().unwrap();
        let next = Region::new(false);
        assert_eq!(next.alloc(8u64) as usize, a);
    }

    #[test]
    // Under Miri, `vm::retire` deallocates the chunk, and Miri may hand its address out again.
    #[cfg_attr(miri, ignore = "vm::retire deallocates under Miri")]
    fn protected_chunks_are_not_reused() {
        let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        let first = Region::new(true);
        let a = first.alloc(1u64) as usize;
        drop(first);
        let second = Region::new(false);
        assert_ne!(second.alloc(2u64) as usize, a);
    }
}
