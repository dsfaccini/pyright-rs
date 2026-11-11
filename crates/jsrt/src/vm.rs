//! The virtual-memory calls behind the heaps (`heap.rs`) and regions (`region.rs`).
//!
//! Miri can neither reserve address space nor change page protection, so under
//! Miri each mapping is an ordinary zeroed allocation, committing is a no-op, and
//! unmapping deallocates: Miri then reports any later use as a use after free.

use std::ptr;

#[cfg(miri)]
const ALIGN: usize = 4096;

#[cfg(miri)]
fn layout(len: usize) -> std::alloc::Layout {
    std::alloc::Layout::from_size_align(len, ALIGN).expect("vm: a mapping length overflows")
}

/// Maps `len` bytes of fresh zeroed memory at an address the kernel picks:
/// readable and writable when `writable`, else reserved only, to commit with
/// [`commit`]. Pages are backed on first touch.
pub(crate) fn map(len: usize, writable: bool) -> Option<*mut u8> {
    #[cfg(miri)]
    {
        let _ = writable;
        // SAFETY: `len` is nonzero.
        let p = unsafe { std::alloc::alloc_zeroed(layout(len)) };
        (!p.is_null()).then_some(p)
    }
    #[cfg(not(miri))]
    {
        let prot = if writable { libc::PROT_READ | libc::PROT_WRITE } else { libc::PROT_NONE };
        // SAFETY: an anonymous private mapping at an address the kernel picks.
        let p = unsafe {
            libc::mmap(ptr::null_mut(), len, prot, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE, -1, 0)
        };
        (p != libc::MAP_FAILED).then_some(p.cast::<u8>())
    }
}

/// Makes reserved pages readable and writable.
pub(crate) fn commit(p: *mut u8, len: usize) -> bool {
    #[cfg(miri)]
    {
        let _ = (p, len);
        true
    }
    #[cfg(not(miri))]
    // SAFETY: `p..p + len` lies in a mapping `map` made.
    unsafe {
        libc::mprotect(p.cast(), len, libc::PROT_READ | libc::PROT_WRITE) == 0
    }
}

/// Returns the pages' contents to the OS; they read as zeros afterwards.
///
/// # Safety
/// `p..p + len` lies in a mapping `map` made, and nothing uses its contents any more.
pub(crate) unsafe fn discard(p: *mut u8, len: usize) {
    #[cfg(miri)]
    // SAFETY: as the caller guarantees.
    unsafe {
        ptr::write_bytes(p, 0, len)
    };
    #[cfg(not(miri))]
    // SAFETY: as the caller guarantees.
    unsafe {
        libc::madvise(p.cast(), len, libc::MADV_DONTNEED)
    };
}

/// Discards the pages and makes them inaccessible, never to be reused, so a later
/// access through a stale pointer faults; under Miri, deallocates the mapping.
///
/// # Safety
/// `p` and `len` are a whole mapping `map` made, and nothing uses it any more.
pub(crate) unsafe fn retire(p: *mut u8, len: usize) {
    #[cfg(miri)]
    // SAFETY: as the caller guarantees.
    unsafe {
        unmap(p, len)
    };
    #[cfg(not(miri))]
    // SAFETY: as the caller guarantees.
    unsafe {
        libc::madvise(p.cast(), len, libc::MADV_DONTNEED);
        libc::mprotect(p.cast(), len, libc::PROT_NONE);
    }
}

/// Unmaps a mapping.
///
/// # Safety
/// `p` and `len` are a whole mapping `map` made, and nothing uses it any more.
pub(crate) unsafe fn unmap(p: *mut u8, len: usize) {
    #[cfg(miri)]
    // SAFETY: `map` allocated `p` with this layout.
    unsafe {
        std::alloc::dealloc(p, layout(len))
    };
    #[cfg(not(miri))]
    // SAFETY: as the caller guarantees.
    unsafe {
        libc::munmap(p.cast(), len)
    };
}

/// The address-space limit (`RLIMIT_AS`), or `usize::MAX`.
pub(crate) fn address_space_limit() -> usize {
    if cfg!(miri) {
        return usize::MAX;
    }
    let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: `getrlimit` writes the struct it is given.
    if unsafe { libc::getrlimit(libc::RLIMIT_AS, &mut limit) } != 0 || limit.rlim_cur == libc::RLIM_INFINITY {
        return usize::MAX;
    }
    usize::try_from(limit.rlim_cur).unwrap_or(usize::MAX)
}

/// Resident bytes of the pages overlapping `[addr, addr + len)`, a mapped range;
/// 0 under Miri.
pub(crate) fn resident(addr: usize, len: usize) -> usize {
    const PAGE: usize = 4096;
    if len == 0 || cfg!(miri) {
        return 0;
    }
    let start = addr & !(PAGE - 1);
    let end = (addr + len).next_multiple_of(PAGE);
    let mut pages = vec![0u8; (end - start) / PAGE];
    // SAFETY: the range is mapped; mincore only reports residency into `pages`.
    if unsafe { libc::mincore(ptr::without_provenance_mut(start), end - start, pages.as_mut_ptr()) } != 0 {
        return 0;
    }
    pages.iter().filter(|&&page| page & 1 != 0).count() * PAGE
}
