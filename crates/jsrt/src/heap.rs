//! Typed-block heaps behind `P`, and the evaluator heap collector
//! (`docs/collector-design.md`).
//!
//! Two address ranges are reserved once per process and cut into 64 KiB
//! blocks. A block belongs to one thread and holds objects of one Rust type in
//! fixed slots; its metadata sits in a side table indexed by block number.
//! Program objects whose type can hold no evaluator handle stay in a bump
//! arena (`arena.rs`): the collector never needs to enumerate them.

use std::any::TypeId;
use std::cell::{Cell, UnsafeCell};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed, Ordering::SeqCst};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Instant;

use crate::collections::FxHashMap;
use crate::trace::Trace;

const BLOCK_SHIFT: u32 = 16;
const BLOCK: usize = 1 << BLOCK_SHIFT;
/// Slot sizes up to this keep the reciprocal slot index exact (offset × slot < 2^32).
pub(crate) const MAX_SLOT: usize = 1 << 14;
const NONE: u32 = u32::MAX;
const PROGRAM_RESERVE: usize = 64 << 30;
const EVAL_RESERVE: usize = 256 << 30;
/// The smallest reservation tried when address space is limited.
const MIN_RESERVE: usize = 256 << 20;
/// Blocks committed (made readable and writable) at a time.
const COMMIT_BLOCKS: usize = 512;
const POISON: u8 = 0xDB;

/// `PYRIGHT_RS_GC`: whether the evaluator heap is collected between files.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GcMode {
    Off,
    /// The default.
    On,
    /// Collect often; poison swept objects, never reuse them, and panic on use.
    Check,
}

struct Config {
    mode: GcMode,
    min_bytes: usize,
    ratio: f64,
    /// `PYRIGHT_RS_GC_LOG`: 0 silent, 1 a line per collection, 2 also a table per class.
    log: u8,
    /// Threads that mark, the collecting thread included (`PYRIGHT_RS_GC_THREADS`).
    threads: usize,
    /// `PYRIGHT_RS_GC_GENERATIONAL`: minor collections trace young objects and the
    /// old objects written since the last collection.
    generational: bool,
    /// A minor collection becomes major once the old objects grew by this share of
    /// their bytes after the last major one (`PYRIGHT_RS_GC_MAJOR_RATIO`).
    major_ratio: f64,
    /// Released blocks kept resident, per range (`PYRIGHT_RS_GC_WARM_MIB`).
    warm_blocks: usize,
}

static CONFIG: OnceLock<Config> = OnceLock::new();
/// When the heap was first used: the collection log's time origin.
static START: OnceLock<Instant> = OnceLock::new();
/// Check mode's hooks on `P` and on program allocation exist only in a build
/// with `--features jsrt/gc-check`: a flag read on every dereference costs
/// about 5% wall time on the samples corpus.
const CHECKS: bool = cfg!(any(test, feature = "gc-check"));
/// Read by every `P` dereference in a `gc-check` build: true in check mode.
static CHECK_HANDLES: AtomicBool = AtomicBool::new(false);

fn env_number<N: std::str::FromStr>(name: &str) -> Option<N> {
    let value = std::env::var(name).ok()?;
    Some(value.parse().unwrap_or_else(|_| panic!("{name} must be a number, not {value:?}")))
}

fn config() -> &'static Config {
    CONFIG.get_or_init(|| {
        START.get_or_init(Instant::now);
        let mode = match std::env::var("PYRIGHT_RS_GC").as_deref() {
            Ok("off") => GcMode::Off,
            Err(_) | Ok("") | Ok("on") => GcMode::On,
            Ok("check") => GcMode::Check,
            Ok(other) => panic!("PYRIGHT_RS_GC must be off, on or check, not {other:?}"),
        };
        let check = mode == GcMode::Check;
        let min_mib: usize = env_number("PYRIGHT_RS_GC_MIN_MIB").unwrap_or(if check { 64 } else { 1024 });
        let ratio: f64 = env_number("PYRIGHT_RS_GC_RATIO").unwrap_or(if check { 0.0 } else { 1.0 });
        if check {
            if !CHECKS {
                panic!("PYRIGHT_RS_GC=check needs a build with --features jsrt/gc-check");
            }
            CHECK_HANDLES.store(true, Relaxed);
        }
        let log = match std::env::var("PYRIGHT_RS_GC_LOG").as_deref() {
            Err(_) | Ok("") | Ok("0") => 0,
            Ok("2") => 2,
            Ok(_) => 1,
        };
        let default_threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(8));
        let threads = env_number("PYRIGHT_RS_GC_THREADS").unwrap_or(default_threads).max(1);
        let generational = match std::env::var("PYRIGHT_RS_GC_GENERATIONAL").as_deref() {
            Ok("0") => false,
            Err(_) | Ok("") | Ok("1") => true,
            Ok(other) => panic!("PYRIGHT_RS_GC_GENERATIONAL must be 0 or 1, not {other:?}"),
        };
        let major_ratio: f64 = env_number("PYRIGHT_RS_GC_MAJOR_RATIO").unwrap_or(1.0);
        let warm_mib: usize = env_number("PYRIGHT_RS_GC_WARM_MIB").unwrap_or(256);
        let warm_blocks = (warm_mib << 20) >> BLOCK_SHIFT;
        Config { mode, min_bytes: min_mib << 20, ratio, log, threads, generational, major_ratio, warm_blocks }
    })
}

/// The collection mode, read from `PYRIGHT_RS_GC` once per process.
pub fn gc_mode() -> GcMode {
    config().mode
}

/// Whether a program object of type `T` goes into a typed block, where the
/// collector scans it as a root. Check mode puts every program type there, so
/// the scan can verify `#[trace(program)]` fields and `&'static` handles.
#[inline]
pub(crate) fn typed_program<T: Trace>() -> bool {
    T::HOLDS_EVAL || (CHECKS && (config().mode == GcMode::Check || CHECK_HANDLES.load(Relaxed)))
}

/// Locks `mutex`. A panic elsewhere never leaves the guarded data half-written,
/// so a poisoned lock is still usable.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---------------------------------------------------------------------------
// reserved ranges and block metadata

/// Per-block metadata. Only the thread that owns the block touches it.
#[repr(C)]
struct BlockMeta {
    name: &'static str,
    trace: TraceFn,
    /// Live, mark and dead bitmaps, `words` words each; `cap` words allocated.
    bits: *mut u64,
    owner: u32,
    slot: u32,
    recip: u32,
    count: u32,
    /// Program blocks: the slots handed out, in order.
    bump: u32,
    /// Program blocks: the slots a collection traced; the ones after them are young.
    scanned: u32,
    words: u32,
    cap: u32,
}

/// Committing `COMMIT_BLOCKS` blocks commits whole pages of the metadata table.
const _: () = assert!(size_of::<BlockMeta>() == 64);

impl BlockMeta {
    #[inline]
    fn index(&self, offset: usize) -> usize {
        ((offset as u64 * self.recip as u64) >> 32) as usize
    }

    /// The next program slot. Program blocks are never swept, so they only bump.
    #[inline]
    fn bump(&mut self, base: usize) -> Option<usize> {
        if self.bump == self.count {
            return None;
        }
        self.bump += 1;
        Some(base + (self.bump as usize - 1) * self.slot as usize)
    }

    /// Evaluator blocks: the slots of bitmap word `w` that are neither live nor dead.
    ///
    /// # Safety
    /// `w < words`, and the block's bitmaps are allocated.
    #[inline]
    unsafe fn free_bits(&self, w: usize) -> u64 {
        let words = self.words as usize;
        let taken = unsafe { *self.bits.add(w) | *self.bits.add(2 * words + w) };
        let tail = self.count as usize - 64 * w;
        let valid = if tail >= 64 { u64::MAX } else { (1u64 << tail) - 1 };
        !taken & valid
    }
}

/// Blocks handed out so far, blocks committed, and released blocks.
struct RangeBlocks {
    next: usize,
    committed: usize,
    /// Released blocks whose pages stay resident, reused first.
    warm: Vec<u32>,
    /// Released blocks whose pages went back to the OS.
    free: Vec<u32>,
}

struct Range {
    name: &'static str,
    base: usize,
    len: usize,
    meta: *mut BlockMeta,
    /// One byte per `CARD` bytes of the range, set by the write barrier.
    cards: usize,
    blocks: Mutex<RangeBlocks>,
}

/// The write barrier marks a card of this many bytes.
const CARD_SHIFT: u32 = 9;
const CARDS_PER_BLOCK: usize = BLOCK >> CARD_SHIFT;

// SAFETY: a block's metadata is touched only by the thread that owns the block;
// `blocks` hands blocks out across threads.
unsafe impl Sync for Range {}
unsafe impl Send for Range {}

impl Range {
    /// Reserves up to `want` bytes, halving until the kernel agrees: an address-space
    /// limit (`ulimit -v`) or strict overcommit can refuse the full size.
    fn reserve(name: &'static str, want: usize, limit: usize) -> Range {
        let mut len = want.min(limit).next_multiple_of(BLOCK);
        loop {
            let meta_len = (len >> BLOCK_SHIFT) * size_of::<BlockMeta>();
            if let Some(base) = map(len + BLOCK) {
                if let Some(meta) = map(meta_len) {
                    if let Some(cards) = map(len >> CARD_SHIFT) {
                        let base = base.next_multiple_of(BLOCK);
                        let blocks = Mutex::new(RangeBlocks { next: 0, committed: 0, warm: Vec::new(), free: Vec::new() });
                        return Range { name, base, len, meta: meta as *mut BlockMeta, cards, blocks };
                    }
                    unsafe { libc::munmap(meta as *mut libc::c_void, meta_len) };
                }
                unsafe { libc::munmap(base as *mut libc::c_void, len + BLOCK) };
            }
            assert!(len > MIN_RESERVE, "collector: cannot reserve address space for the {name} heap");
            len /= 2;
        }
    }

    #[inline]
    fn block_base(&self, block: u32) -> usize {
        self.base + ((block as usize) << BLOCK_SHIFT)
    }

    #[allow(clippy::mut_from_ref)]
    #[inline]
    unsafe fn meta(&self, block: u32) -> &mut BlockMeta {
        unsafe { &mut *self.meta.add(block as usize) }
    }

    /// The cards of a committed block.
    #[allow(clippy::mut_from_ref)]
    unsafe fn cards(&self, block: u32) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut((self.cards + block as usize * CARDS_PER_BLOCK) as *mut u8, CARDS_PER_BLOCK) }
    }

    /// A free block: a released one, or the next one, committing more when needed.
    fn take_block(&self) -> u32 {
        let mut blocks = lock(&self.blocks);
        if let Some(block) = blocks.warm.pop().or_else(|| blocks.free.pop()) {
            return block;
        }
        let block = blocks.next;
        if block == blocks.committed {
            let count = COMMIT_BLOCKS.min((self.len >> BLOCK_SHIFT) - block);
            let committed = count > 0
                && protect(self.block_base(block as u32), count << BLOCK_SHIFT)
                && protect(self.meta as usize + block * size_of::<BlockMeta>(), count * size_of::<BlockMeta>())
                && protect(self.cards + block * CARDS_PER_BLOCK, count * CARDS_PER_BLOCK);
            if !committed {
                drop(blocks);
                panic!("collector: the {} heap reservation of {} MiB is full or cannot be committed", self.name, self.len >> 20);
            }
            blocks.committed += count;
        }
        blocks.next += 1;
        block as u32
    }

    /// Keeps up to `PYRIGHT_RS_GC_WARM_MIB` of released blocks resident, so the next
    /// allocations into them do not fault; returns the pages of the rest to the OS.
    fn release_block(&self, block: u32) {
        let mut blocks = lock(&self.blocks);
        if blocks.warm.len() < config().warm_blocks {
            blocks.warm.push(block);
            return;
        }
        drop(blocks);
        // SAFETY: the block is committed, owned by this thread, and holds no live object.
        unsafe { libc::madvise(self.block_base(block) as *mut libc::c_void, BLOCK, libc::MADV_DONTNEED) };
        lock(&self.blocks).free.push(block);
    }
}

/// Reserves address space without committing it.
fn map(len: usize) -> Option<usize> {
    // SAFETY: an anonymous private mapping at an address the kernel picks.
    let p = unsafe {
        libc::mmap(
            ptr::null_mut(),
            len,
            libc::PROT_NONE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
            -1,
            0,
        )
    };
    (p != libc::MAP_FAILED).then_some(p as usize)
}

/// Makes reserved pages readable and writable; they are backed on first touch.
fn protect(addr: usize, len: usize) -> bool {
    // SAFETY: `addr..addr + len` lies in a mapping `map` reserved.
    unsafe { libc::mprotect(addr as *mut libc::c_void, len, libc::PROT_READ | libc::PROT_WRITE) == 0 }
}

/// The address-space limit (`RLIMIT_AS`), or `usize::MAX`.
fn address_space_limit() -> usize {
    let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: `getrlimit` writes the struct it is given.
    if unsafe { libc::getrlimit(libc::RLIMIT_AS, &mut limit) } != 0 || limit.rlim_cur == libc::RLIM_INFINITY {
        return usize::MAX;
    }
    usize::try_from(limit.rlim_cur).unwrap_or(usize::MAX)
}

struct Ranges {
    program: Range,
    eval: Range,
}

static RANGES: OnceLock<Ranges> = OnceLock::new();

/// Where the write barrier marks cards. The lengths stay 0, so the barrier marks
/// nothing, unless collection is generational.
struct Barrier {
    eval_base: AtomicUsize,
    eval_len: AtomicUsize,
    eval_cards: AtomicUsize,
    program_base: AtomicUsize,
    program_len: AtomicUsize,
    program_cards: AtomicUsize,
}

static BARRIER: Barrier = Barrier {
    eval_base: AtomicUsize::new(0),
    eval_len: AtomicUsize::new(0),
    eval_cards: AtomicUsize::new(0),
    program_base: AtomicUsize::new(0),
    program_len: AtomicUsize::new(0),
    program_cards: AtomicUsize::new(0),
};

/// The write barrier: the cell at `addr` was written, or is about to be, with a
/// value that can hold an evaluator handle. When the cell lies in a typed block,
/// marks its card, so a minor collection traces the objects on it. `jsrt::cell`
/// calls it; a cell elsewhere (the stack, a buffer) needs none, because no heap
/// object's cell sits in a buffer (`Trace::CELL_OUT_OF_LINE`).
#[inline(always)]
pub fn remember(addr: usize) {
    let offset = addr.wrapping_sub(BARRIER.eval_base.load(Relaxed));
    if offset < BARRIER.eval_len.load(Relaxed) {
        // SAFETY: the card table covers the committed part of the range, and the cell is in a committed block.
        unsafe { *((BARRIER.eval_cards.load(Relaxed) + (offset >> CARD_SHIFT)) as *mut u8) = 1 };
        return;
    }
    let offset = addr.wrapping_sub(BARRIER.program_base.load(Relaxed));
    if offset < BARRIER.program_len.load(Relaxed) {
        // SAFETY: as above.
        unsafe { *((BARRIER.program_cards.load(Relaxed) + (offset >> CARD_SHIFT)) as *mut u8) = 1 };
    }
}

fn ranges() -> &'static Ranges {
    RANGES.get_or_init(|| {
        // Under an address-space limit, leave most of it to the program heap's bump
        // arena, the system allocator and the stack.
        let limit = address_space_limit();
        let ranges = Ranges {
            program: Range::reserve("program", PROGRAM_RESERVE, limit / 16),
            eval: Range::reserve("evaluator", EVAL_RESERVE, limit / 4),
        };
        if config().generational && config().mode != GcMode::Off {
            BARRIER.eval_base.store(ranges.eval.base, Relaxed);
            BARRIER.eval_cards.store(ranges.eval.cards, Relaxed);
            BARRIER.eval_len.store(ranges.eval.len, Relaxed);
            BARRIER.program_base.store(ranges.program.base, Relaxed);
            BARRIER.program_cards.store(ranges.program.cards, Relaxed);
            BARRIER.program_len.store(ranges.program.len, Relaxed);
        }
        ranges
    })
}

// ---------------------------------------------------------------------------
// type registry

#[derive(Clone, Copy)]
struct TypeInfo {
    name: &'static str,
    holds_eval: bool,
    slot: u32,
    recip: u32,
    count: u32,
    drop: unsafe fn(usize),
    prefetch_owned: unsafe fn(usize),
    trace: TraceFn,
}

#[derive(Default)]
struct Registry {
    ids: HashMap<TypeId, u32>,
    infos: Vec<TypeInfo>,
}

static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

/// A type's process-wide allocator id, cached in a static the derive emits.
pub struct ClassSlot(AtomicU32);

impl ClassSlot {
    #[allow(clippy::new_without_default)]
    pub const fn new() -> ClassSlot {
        ClassSlot(AtomicU32::new(NONE))
    }
}

unsafe fn drop_erased<T>(addr: usize) {
    unsafe { ptr::drop_in_place(addr as *mut T) }
}

unsafe fn prefetch_owned_erased<T: Trace>(addr: usize) {
    unsafe { (*(addr as *const T)).prefetch_owned() }
}

unsafe fn trace_erased<T: Trace>(addr: usize, t: &mut Tracer) {
    unsafe { (*(addr as *const T)).trace(t) }
}

fn register<T: Trace + 'static>() -> u32 {
    let name = std::any::type_name::<T>();
    let slot = size_of::<T>().max(8).next_multiple_of(align_of::<T>());
    assert!(slot <= MAX_SLOT, "collector: {name} is {slot} bytes; slots go up to {MAX_SLOT}");
    let info = TypeInfo {
        name,
        holds_eval: T::HOLDS_EVAL,
        slot: slot as u32,
        recip: (1u64 << 32).div_ceil(slot as u64) as u32,
        count: (BLOCK / slot) as u32,
        drop: drop_erased::<T>,
        prefetch_owned: prefetch_owned_erased::<T>,
        trace: trace_erased::<T>,
    };
    let mut registry = lock(&REGISTRY);
    let registry = registry.get_or_insert_with(Registry::default);
    if let Some(&id) = registry.ids.get(&TypeId::of::<T>()) {
        return id;
    }
    let id = registry.infos.len() as u32;
    registry.infos.push(info);
    registry.ids.insert(TypeId::of::<T>(), id);
    id
}

fn type_info(id: u32) -> TypeInfo {
    lock(&REGISTRY).as_ref().unwrap().infos[id as usize]
}

// ---------------------------------------------------------------------------
// per-thread heap

struct Class {
    info: TypeInfo,
    current: u32,
    /// Evaluator classes: where `current` hands out slots next. `free` holds the
    /// slots of bitmap word `word` not handed out yet, and `live` points at that
    /// word's live bits. Allocation never reads a slot: after a sweep it is cold.
    word: u32,
    free: u64,
    live: *mut u64,
    base: usize,
    blocks: Vec<u32>,
    partial: Vec<u32>,
}

struct ThreadHeap {
    owner: u32,
    generic: FxHashMap<TypeId, u32>,
    program: Vec<Option<Class>>,
    eval: Vec<Option<Class>>,
    roots: Vec<fn(&mut Tracer)>,
    /// Evaluator slot bytes allocated since the last collection.
    since_gc: usize,
    /// Evaluator slot bytes live after the last collection.
    live_bytes: usize,
    collections: u32,
    /// Generational: evaluator slot bytes live after the last major collection.
    major_live_bytes: usize,
    /// The next collection must be major: a mark was interrupted, so mark bits
    /// no longer tell old objects apart.
    force_major: bool,
    /// The last sweep left survivors marked (generational), so a major collection
    /// clears the marks first.
    sticky_marks: bool,
}

/// The heap, and whether something is using it: allocation, root registration
/// and collection must not nest.
struct HeapCell {
    busy: Cell<bool>,
    heap: UnsafeCell<ThreadHeap>,
}

static NEXT_OWNER: AtomicU32 = AtomicU32::new(1);

thread_local! {
    static HEAP: &'static HeapCell = Box::leak(Box::new(HeapCell {
        busy: Cell::new(false),
        heap: UnsafeCell::new(ThreadHeap {
            owner: OWNER.with(|owner| *owner),
            generic: FxHashMap::default(),
            program: Vec::new(),
            eval: Vec::new(),
            roots: Vec::new(),
            since_gc: 0,
            live_bytes: 0,
            collections: 0,
            major_live_bytes: 0,
            force_major: true,
            sticky_marks: false,
        }),
    }));
    /// This thread's block owner id.
    static OWNER: u32 = NEXT_OWNER.fetch_add(1, Relaxed);
}

struct Busy<'a>(&'a Cell<bool>);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// Runs `f` on this thread's heap. Panics when `f`, or a root, `Trace` impl
/// or drop it runs, uses the heap again: an allocation inside a collection.
#[inline]
fn with_heap<R>(f: impl FnOnce(&mut ThreadHeap) -> R) -> R {
    HEAP.with(|cell| {
        assert!(!cell.busy.get(), "collector: the heap was used inside an allocation or a collection");
        cell.busy.set(true);
        let _busy = Busy(&cell.busy);
        // SAFETY: `busy` guarantees this is the only `&mut` to the heap.
        f(unsafe { &mut *cell.heap.get() })
    })
}

/// A slot for one `T` in a typed block of the evaluator heap (`eval`) or the program heap.
#[inline]
pub(crate) fn alloc<T: Trace + 'static>(eval: bool) -> *mut T {
    with_heap(|heap| unsafe { heap.alloc::<T>(eval) }) as *mut T
}

impl ThreadHeap {
    #[inline]
    fn type_id<T: Trace + 'static>(&mut self) -> u32 {
        match T::class_slot() {
            Some(slot) => match slot.0.load(Relaxed) {
                NONE => {
                    let id = register::<T>();
                    slot.0.store(id, Relaxed);
                    id
                }
                id => id,
            },
            None => *self.generic.entry(TypeId::of::<T>()).or_insert_with(register::<T>),
        }
    }

    #[inline]
    unsafe fn alloc<T: Trace + 'static>(&mut self, eval: bool) -> usize {
        let id = self.type_id::<T>() as usize;
        if eval {
            if let Some(Some(class)) = self.eval.get_mut(id)
                && class.free != 0
            {
                let addr = unsafe { class.take() };
                self.since_gc += class.info.slot as usize;
                return addr;
            }
        } else if let Some(Some(class)) = self.program.get_mut(id)
            && class.current != NONE
        {
            let program = &ranges().program;
            if let Some(addr) = unsafe { program.meta(class.current) }.bump(program.block_base(class.current)) {
                return addr;
            }
        }
        unsafe { self.alloc_slow(id, eval) }
    }

    #[cold]
    unsafe fn alloc_slow(&mut self, id: usize, eval: bool) -> usize {
        let owner = self.owner;
        let range = if eval { &ranges().eval } else { &ranges().program };
        let classes = if eval { &mut self.eval } else { &mut self.program };
        if classes.len() <= id {
            classes.resize_with(id + 1, || None);
        }
        let class = classes[id].get_or_insert_with(|| Class {
            info: type_info(id as u32),
            current: NONE,
            word: 0,
            free: 0,
            live: ptr::null_mut(),
            base: 0,
            blocks: Vec::new(),
            partial: Vec::new(),
        });
        if !eval {
            loop {
                if class.current != NONE
                    && let Some(addr) = unsafe { range.meta(class.current) }.bump(range.block_base(class.current))
                {
                    return addr;
                }
                class.current = unsafe { new_block(range, class, owner, false) };
            }
        }
        loop {
            if class.current != NONE {
                // SAFETY: `current` is a block of this class, owned by this thread.
                let meta = unsafe { range.meta(class.current) };
                while class.free == 0 && class.word + 1 < meta.words {
                    class.word += 1;
                    class.free = unsafe { meta.free_bits(class.word as usize) };
                }
                if class.free != 0 {
                    class.live = unsafe { meta.bits.add(class.word as usize) };
                    self.since_gc += class.info.slot as usize;
                    return unsafe { class.take() };
                }
            }
            let block = match class.partial.pop() {
                Some(block) => block,
                None => unsafe { new_block(range, class, owner, true) },
            };
            class.current = block;
            class.base = range.block_base(block);
            class.word = 0;
            class.free = unsafe { range.meta(block).free_bits(0) };
            class.live = unsafe { range.meta(block).bits };
        }
    }
}

impl Class {
    /// Hands out the lowest free slot of the cursor's word and sets its live bit.
    ///
    /// # Safety
    /// `free != 0`, and `live` points at the live bits of word `word` of `current`.
    #[inline]
    unsafe fn take(&mut self) -> usize {
        let j = self.free.trailing_zeros();
        self.free &= self.free - 1;
        unsafe { *self.live |= 1 << j };
        self.base + (self.word as usize * 64 + j as usize) * self.info.slot as usize
    }
}

unsafe fn new_block(range: &Range, class: &mut Class, owner: u32, eval: bool) -> u32 {
    let block = range.take_block();
    let info = &class.info;
    let meta = unsafe { range.meta(block) };
    meta.name = info.name;
    meta.trace = info.trace;
    meta.owner = owner;
    meta.slot = info.slot;
    meta.recip = info.recip;
    meta.count = info.count;
    meta.bump = 0;
    meta.scanned = 0;
    unsafe { range.cards(block) }.fill(0);
    if eval {
        let words = info.count.div_ceil(64);
        let need = 3 * words;
        if meta.cap < need {
            if !meta.bits.is_null() {
                // SAFETY: allocated below with this capacity.
                drop(unsafe { Vec::from_raw_parts(meta.bits, meta.cap as usize, meta.cap as usize) });
            }
            let mut bits = std::mem::ManuallyDrop::new(vec![0u64; need as usize]);
            meta.bits = bits.as_mut_ptr();
            meta.cap = need;
        } else {
            unsafe { ptr::write_bytes(meta.bits, 0, need as usize) };
        }
        meta.words = words;
    }
    class.blocks.push(block);
    block
}

/// Registers a root: a function that traces a thread-local holding evaluator handles.
pub fn add_root(root: fn(&mut Tracer)) {
    with_heap(|heap| heap.roots.push(root));
}

/// Evaluator slot bytes live after the last collection.
pub fn eval_heap_live_bytes() -> usize {
    with_heap(|heap| heap.live_bytes)
}

// ---------------------------------------------------------------------------
// handle checks (check mode)

/// Panics when `addr` is a swept evaluator object. Check mode only.
#[inline(always)]
pub(crate) fn check_handle(addr: usize) {
    if CHECKS && CHECK_HANDLES.load(Relaxed) {
        check_handle_slow(addr);
    }
}

#[cold]
#[inline(never)]
fn check_handle_slow(addr: usize) {
    let range = &ranges().eval;
    let offset = addr.wrapping_sub(range.base);
    if offset >= range.len {
        return;
    }
    // SAFETY: a handle into the evaluator range points into a committed block; its
    // metadata is read only when this thread owns it.
    let meta = unsafe { range.meta((offset >> BLOCK_SHIFT) as u32) };
    if meta.owner != OWNER.with(|owner| *owner) || meta.bits.is_null() {
        return;
    }
    let i = meta.index(offset & (BLOCK - 1));
    let dead = unsafe { *meta.bits.add(2 * meta.words as usize + i / 64) } & (1 << (i % 64));
    assert!(dead == 0, "collector check: use of a swept {}", meta.name);
}

// ---------------------------------------------------------------------------
// collection

type TraceFn = unsafe fn(usize, &mut Tracer);

/// Finds the evaluator objects reachable from the roots.
pub struct Tracer {
    stack: Vec<(usize, TraceFn)>,
    check: bool,
    program_only: u32,
    owner: u32,
    eval_base: usize,
    eval_len: usize,
    meta: *mut BlockMeta,
    /// The work of a parallel mark, or null while marking on one thread.
    shared: *const MarkShared,
}

impl Tracer {
    /// Check mode: visit every field and verify the claims about handles.
    #[inline]
    pub fn checking(&self) -> bool {
        self.check
    }

    /// Marks the object at `addr` when it is on the evaluator heap.
    #[inline]
    pub fn mark(&mut self, addr: usize) {
        let offset = addr.wrapping_sub(self.eval_base);
        if offset < self.eval_len {
            self.mark_eval(offset);
        }
    }

    #[inline]
    fn mark_eval(&mut self, offset: usize) {
        if self.check {
            assert!(self.program_only == 0, "collector check: a #[trace(program)] field holds an evaluator handle");
        }
        // SAFETY: checked below that this thread owns the block.
        let meta = unsafe { &*self.meta.add(offset >> BLOCK_SHIFT) };
        assert!(meta.owner == self.owner, "collector: a handle points into an evaluator block this thread does not own");
        let i = meta.index(offset & (BLOCK - 1));
        let (w, bit, words) = (i / 64, 1u64 << (i % 64), meta.words as usize);
        // SAFETY: `i` is a slot of the block, and its bitmaps hold `words` words each.
        // In a parallel mark every thread sets mark bits atomically.
        unsafe {
            let mark = meta.bits.add(words + w);
            if self.shared.is_null() {
                if *mark & bit != 0 {
                    return;
                }
                if *meta.bits.add(w) & bit == 0 {
                    dangling(meta, w, bit);
                }
                *mark |= bit;
            } else {
                let mark = AtomicU64::from_ptr(mark);
                if mark.load(Relaxed) & bit != 0 {
                    return;
                }
                if *meta.bits.add(w) & bit == 0 {
                    dangling(meta, w, bit);
                }
                if mark.fetch_or(bit, Relaxed) & bit != 0 {
                    return;
                }
            }
        }
        let addr = self.eval_base + (offset & !(BLOCK - 1)) + i * meta.slot as usize;
        self.stack.push((addr, meta.trace));
    }

    /// Check mode: traces a `#[trace(program)]` field and panics on an evaluator handle in it.
    pub fn program_only<T: ?Sized + Trace>(&mut self, value: &T) {
        self.program_only += 1;
        value.trace(self);
        self.program_only -= 1;
    }

    /// Check mode: panics when a handle the type claims is never on the evaluator heap is.
    pub fn expect_program(&self, addr: usize, what: &str) {
        assert!(
            addr.wrapping_sub(self.eval_base) >= self.eval_len,
            "collector check: a {what} handle that its type keeps off the evaluator heap points into it"
        );
    }

    /// Traces every object on the mark stack. Objects pass through a small FIFO
    /// whose entries are prefetched, so the cache misses of several objects overlap.
    fn drain(&mut self) {
        const AHEAD: usize = 8;
        let mut ring = [(0usize, None::<TraceFn>); AHEAD];
        let (mut head, mut len) = (0usize, 0usize);
        loop {
            while len < AHEAD {
                let Some((addr, trace)) = self.stack.pop() else { break };
                prefetch(addr);
                ring[(head + len) % AHEAD] = (addr, Some(trace));
                len += 1;
            }
            if len == 0 {
                return;
            }
            let (addr, trace) = ring[head];
            head = (head + 1) % AHEAD;
            len -= 1;
            // SAFETY: `addr` is a live slot of the type `trace` was made for.
            unsafe { trace.unwrap_unchecked()(addr, self) };
            if !self.shared.is_null() && self.stack.len() >= 2 {
                self.share();
            }
        }
    }

    /// Parallel mark: while a thread waits for work, hands it the top half of the
    /// mark stack, at most `PACKET` entries.
    #[inline]
    fn share(&mut self) {
        // SAFETY: `shared` outlives the mark.
        let shared = unsafe { &*self.shared };
        let hungry = shared.hungry.load(Relaxed);
        if hungry == 0 || shared.pooled.load(Relaxed) >= hungry {
            return;
        }
        let n = (self.stack.len() / 2).min(PACKET);
        let packet = self.stack.split_off(self.stack.len() - n);
        lock(&shared.pool).packets.push(packet);
        shared.pooled.fetch_add(1, SeqCst);
        shared.wake.notify_one();
    }
}

/// Mark-stack entries handed between threads at a time.
const PACKET: usize = 512;
/// Root objects a marking thread takes at a time.
const ROOT_CHUNK: usize = 64;

/// The work of a parallel mark: root objects to scan, donated mark-stack
/// packets, and the threads waiting for one.
struct MarkShared {
    /// Root objects, taken `ROOT_CHUNK` at a time.
    roots: Vec<(usize, TraceFn)>,
    next_root: AtomicUsize,
    pool: Mutex<Pool>,
    /// Signalled when a packet arrives and when the mark ends.
    wake: std::sync::Condvar,
    /// Mirrors of `Pool`'s counts, read without the lock.
    pooled: AtomicUsize,
    hungry: AtomicUsize,
    threads: usize,
    abort: AtomicBool,
    panic: Mutex<Option<Box<dyn std::any::Any + Send>>>,
}

#[derive(Default)]
struct Pool {
    packets: Vec<Vec<(usize, TraceFn)>>,
    /// Threads with no work; the mark ends when all of them have none.
    idle: usize,
    done: bool,
}

/// One thread's part of a parallel mark: scan root objects and trace until every
/// thread runs out of work.
fn mark_work(t: &mut Tracer, shared: &MarkShared) {
    'work: loop {
        t.drain();
        if shared.abort.load(Relaxed) {
            break;
        }
        let start = shared.next_root.fetch_add(ROOT_CHUNK, Relaxed).min(shared.roots.len());
        let chunk = &shared.roots[start..(start + ROOT_CHUNK).min(shared.roots.len())];
        if !chunk.is_empty() {
            for &(addr, trace) in chunk {
                // SAFETY: a root entry pairs an object with the trace function of its type.
                unsafe { trace(addr, t) };
            }
            continue;
        }
        let mut pool = lock(&shared.pool);
        pool.idle += 1;
        loop {
            if shared.abort.load(Relaxed) || pool.done {
                break 'work;
            }
            if let Some(packet) = pool.packets.pop() {
                pool.idle -= 1;
                shared.hungry.store(pool.idle, SeqCst);
                shared.pooled.fetch_sub(1, SeqCst);
                drop(pool);
                t.stack.extend(packet);
                continue 'work;
            }
            if pool.idle == shared.threads {
                pool.done = true;
                shared.wake.notify_all();
                break 'work;
            }
            shared.hungry.store(pool.idle, SeqCst);
            pool = shared.wake.wait(pool).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// Runs `mark_work` on this thread and on the helpers, catching a panic in any
/// of them; the first one is kept and the others stop.
fn mark_work_caught(t: &mut Tracer, shared: &MarkShared) {
    if let Err(panic) = catch_unwind(AssertUnwindSafe(|| mark_work(t, shared))) {
        shared.abort.store(true, SeqCst);
        lock(&shared.panic).get_or_insert(panic);
        let _pool = lock(&shared.pool);
        shared.wake.notify_all();
    }
}

/// What a helper thread needs to build its `Tracer`.
#[derive(Clone, Copy)]
struct MarkJob {
    shared: usize,
    owner: u32,
    check: bool,
    eval_base: usize,
    eval_len: usize,
    meta: usize,
}

struct HelperState {
    generation: u64,
    wanted: usize,
    running: usize,
    job: Option<MarkJob>,
}

/// Threads that help mark, parked between collections.
struct Helpers {
    state: Mutex<HelperState>,
    start: std::sync::Condvar,
    done: std::sync::Condvar,
    spawned: Mutex<usize>,
}

static HELPERS: Helpers = Helpers {
    state: Mutex::new(HelperState { generation: 0, wanted: 0, running: 0, job: None }),
    start: std::sync::Condvar::new(),
    done: std::sync::Condvar::new(),
    spawned: Mutex::new(0),
};

impl Helpers {
    /// Runs `job` on `n` helper threads and `main` on this one; returns when all finish.
    fn run(&'static self, n: usize, job: MarkJob, main: impl FnOnce()) {
        {
            let mut spawned = lock(&self.spawned);
            while *spawned < n {
                let index = *spawned;
                std::thread::Builder::new()
                    .name(format!("gc-mark-{index}"))
                    .spawn(move || self.serve(index))
                    .expect("collector: cannot start a marking thread");
                *spawned += 1;
            }
        }
        {
            let mut state = lock(&self.state);
            state.generation += 1;
            state.wanted = n;
            state.running = n;
            state.job = Some(job);
        }
        self.start.notify_all();
        main();
        let mut state = lock(&self.state);
        while state.running > 0 {
            state = self.done.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
        state.job = None;
    }

    fn serve(&self, index: usize) {
        let mut seen = 0;
        loop {
            let job = {
                let mut state = lock(&self.state);
                while state.generation == seen {
                    state = self.start.wait(state).unwrap_or_else(PoisonError::into_inner);
                }
                seen = state.generation;
                if index >= state.wanted {
                    continue;
                }
                state.job.expect("collector: a marking thread started without a job")
            };
            // SAFETY: the collecting thread keeps `shared` alive until every helper reports done.
            let shared = unsafe { &*(job.shared as *const MarkShared) };
            let mut t = Tracer {
                stack: Vec::new(),
                    check: job.check,
                program_only: 0,
                owner: job.owner,
                eval_base: job.eval_base,
                eval_len: job.eval_len,
                meta: job.meta as *mut BlockMeta,
                shared,
            };
            // A trace function must not allocate: on a helper, an allocation would
            // land on the helper's own heap, so its heap counts as in use too.
            HEAP.with(|cell| cell.busy.set(true));
            mark_work_caught(&mut t, shared);
            HEAP.with(|cell| cell.busy.set(false));
            let mut state = lock(&self.state);
            state.running -= 1;
            if state.running == 0 {
                self.done.notify_all();
            }
        }
    }
}

/// Panics on a handle to a slot that holds no object: a handle the collector missed.
#[cold]
#[inline(never)]
fn dangling(meta: &BlockMeta, w: usize, bit: u64) -> ! {
    // SAFETY: the dead bitmap follows the live and mark bitmaps.
    let swept = unsafe { *meta.bits.add(2 * meta.words as usize + w) } & bit != 0;
    panic!("collector: a reachable handle points to a {} {}", if swept { "swept" } else { "free" }, meta.name);
}

#[inline(always)]
pub(crate) fn prefetch(addr: usize) {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: a prefetch never faults.
    unsafe {
        std::arch::x86_64::_mm_prefetch(addr as *const i8, std::arch::x86_64::_MM_HINT_T0);
    }
    #[cfg(not(target_arch = "x86_64"))]
    let _ = addr;
}

/// Collects when the evaluator heap grew by `max(ratio × live bytes, floor)`
/// since the last collection, unless `PYRIGHT_RS_GC` is off. Call only where
/// no evaluator frame is live: before a file in the check loop. Panics when a
/// root, `Trace` impl or drop allocates.
pub fn collect_if_due() -> bool {
    let config = config();
    if config.mode == GcMode::Off {
        return false;
    }
    let check = config.mode == GcMode::Check;
    with_heap(|heap| {
        // Generational: a minor collection every `min_bytes`, a major one once the
        // old objects grew by `major_ratio`. Otherwise a full one at `max(ratio × live, min_bytes)`.
        let threshold = if config.generational {
            config.min_bytes
        } else {
            ((heap.live_bytes as f64 * config.ratio) as usize).max(config.min_bytes)
        };
        if heap.since_gc == 0 || heap.since_gc < threshold {
            return false;
        }
        let old_limit = (heap.major_live_bytes as f64 * (1.0 + config.major_ratio)) as usize;
        let minor = config.generational && !heap.force_major && heap.live_bytes < old_limit.max(config.min_bytes);
        heap.collect(Collection { check, log: config.log, threads: config.threads, minor, generational: config.generational });
        true
    })
}

/// Collects now, in check mode when `check`. Same precondition as [`collect_if_due`].
pub fn collect(check: bool) {
    if check {
        CHECK_HANDLES.store(true, Relaxed);
    }
    let log = config().log;
    let threads = config().threads;
    let generational = config().generational;
    with_heap(|heap| heap.collect(Collection { check, log, threads, minor: false, generational }));
}

thread_local! {
    /// Bytes this thread allocated through [`CountingAlloc`] less the bytes it freed.
    static MALLOC_BYTES: Cell<isize> = const { Cell::new(0) };
}

/// A global allocator that counts the bytes each thread holds in `A`, for the
/// collection log: the buffers evaluator objects own live there.
pub struct CountingAlloc<A>(pub A);

#[inline]
fn count_malloc(bytes: isize) {
    // A const thread-local without a destructor stays usable while the thread exits.
    MALLOC_BYTES.with(|n| n.set(n.get() + bytes));
}

// SAFETY: forwards every call to `A`; counting never allocates.
unsafe impl<A: std::alloc::GlobalAlloc> std::alloc::GlobalAlloc for CountingAlloc<A> {
    #[inline]
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        count_malloc(layout.size() as isize);
        unsafe { self.0.alloc(layout) }
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        count_malloc(layout.size() as isize);
        unsafe { self.0.alloc_zeroed(layout) }
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        count_malloc(-(layout.size() as isize));
        unsafe { self.0.dealloc(ptr, layout) }
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        count_malloc(new_size as isize - layout.size() as isize);
        unsafe { self.0.realloc(ptr, layout, new_size) }
    }
}

/// Bytes this thread holds in the global allocator when it is a [`CountingAlloc`], else 0.
fn malloc_bytes() -> usize {
    MALLOC_BYTES.with(Cell::get).max(0) as usize
}

/// The process's resident set size in bytes.
fn rss_bytes() -> usize {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let pages: usize = statm.split_whitespace().nth(1).and_then(|n| n.parse().ok()).unwrap_or(0);
    // SAFETY: `sysconf` only reads a system constant.
    pages * unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize
}

/// Resident bytes of the mappings that start inside `range`.
fn rss_in(range: &Range) -> usize {
    let smaps = std::fs::read_to_string("/proc/self/smaps").unwrap_or_default();
    let (mut inside, mut total) = (false, 0usize);
    for line in smaps.lines() {
        if let Some((start, _)) = line.split_once('-')
            && let Ok(start) = usize::from_str_radix(start, 16)
        {
            inside = start.wrapping_sub(range.base) < range.len;
        } else if inside && let Some(kib) = line.strip_prefix("Rss:") {
            total += kib.trim().trim_end_matches("kB").trim().parse::<usize>().unwrap_or(0) << 10;
        }
    }
    total
}

/// The sweep prefetches the buffers of the dead object this many after the one it drops.
const SWEEP_AHEAD: usize = 4;

/// Calls `f` once for every slot below `limit` that overlaps a marked card.
fn for_card_slots(cards: &[u8], slot: usize, limit: usize, mut f: impl FnMut(usize)) {
    let mut next = 0;
    for (c, &card) in cards.iter().enumerate() {
        if card == 0 {
            continue;
        }
        let first = ((c << CARD_SHIFT) / slot).max(next);
        let last = ((((c + 1) << CARD_SHIFT) - 1) / slot + 1).min(limit);
        for i in first..last {
            f(i);
        }
        next = next.max(last);
    }
}

/// Per-class counts of one sweep, for `PYRIGHT_RS_GC_LOG=2`.
struct ClassStat {
    name: &'static str,
    slot: usize,
    live: usize,
    freed: usize,
}

/// How one collection runs.
#[derive(Clone, Copy)]
struct Collection {
    check: bool,
    log: u8,
    threads: usize,
    /// Generational only: trace from young objects and written cards, keep old objects.
    minor: bool,
    /// Mark bits survive the sweep, so survivors are old at the next collection.
    generational: bool,
}

impl ThreadHeap {
    /// A collection: major (`minor` false) marks everything the roots reach; minor
    /// marks only from young objects and the old objects on written cards, and
    /// keeps every old object (`docs/collector-design.md`, "generational collection").
    fn collect(&mut self, c: Collection) {
        let Collection { check, log, threads, minor, generational } = c;
        let start = Instant::now();
        let before = if log > 0 { (rss_bytes(), malloc_bytes()) } else { (0, 0) };
        let ranges = ranges();
        let mut t = Tracer {
            stack: Vec::new(),
            check,
            program_only: 0,
            owner: self.owner,
            eval_base: ranges.eval.base,
            eval_len: ranges.eval.len,
            meta: ranges.eval.meta,
            shared: ptr::null(),
        };

        let roots = self.root_objects(check, minor);
        let scanned = roots.len();
        // A panic while marking (check mode's assertions, a borrowed RefCell) must not
        // leave mark bits behind: the next collection would skip those objects' children.
        // After it, mark bits no longer tell old objects apart, so the next collection is major.
        let marked = catch_unwind(AssertUnwindSafe(|| {
            if !minor && self.sticky_marks {
                self.clear_marks();
            }
            let old = if check { self.marks() } else { Vec::new() };
            self.mark_from(&mut t, &roots, threads);
            if check && threads > 1 {
                let parallel = self.marks();
                self.set_marks(&old);
                self.mark_from(&mut t, &roots, 1);
                self.compare_marks(&parallel, "the parallel mark differs from the serial one");
            }
            if check && minor {
                self.verify_minor(&mut t);
            }
        }));
        if let Err(panic) = marked {
            self.clear_marks();
            self.force_major = true;
            resume_unwind(panic);
        }
        let mark_ms = start.elapsed().as_millis();

        // A panic while sweeping leaves blocks half-swept; nothing can be trusted after it.
        let mut stats = Vec::new();
        let swept = catch_unwind(AssertUnwindSafe(|| self.sweep(check, generational, (log > 1).then_some(&mut stats))));
        let Ok((freed, live_bytes)) = swept else {
            eprintln!("collector: a drop panicked during the sweep; aborting");
            std::process::abort();
        };
        self.sticky_marks = generational;
        if generational {
            self.clear_cards();
        }

        self.collections += 1;
        if log > 0 {
            let blocks = |classes: &[Option<Class>]| classes.iter().flatten().map(|c| c.blocks.len()).sum::<usize>() << BLOCK_SHIFT;
            // A thread that allocated holds some bytes, so 0 before and after means nothing counts.
            let malloc = match (before.1, malloc_bytes()) {
                (0, 0) => "not counted".to_string(),
                (before, after) => format!("{} -> {} MiB", before >> 20, after >> 20),
            };
            eprintln!(
                "gc {}{}: at {:.1} s, {} ms ({} ms mark), {} root objects scanned, {} objects freed, \
                 evaluator slots {} MiB live (+{} MiB allocated since the last collection) in {} MiB of blocks, \
                 typed program blocks {} MiB, program arena {} MiB in {} MiB of chunks, malloc {}, \
                 rss {} -> {} MiB (evaluator range {} MiB, program range {} MiB)",
                self.collections,
                if minor { " minor" } else if generational { " major" } else { "" },
                START.get_or_init(Instant::now).elapsed().as_secs_f64(),
                start.elapsed().as_millis(),
                mark_ms,
                scanned,
                freed,
                live_bytes >> 20,
                self.since_gc >> 20,
                blocks(&self.eval) >> 20,
                blocks(&self.program) >> 20,
                crate::arena::arena_allocated_bytes() >> 20,
                crate::arena::arena_chunk_bytes() >> 20,
                malloc,
                before.0 >> 20,
                rss_bytes() >> 20,
                rss_in(&ranges.eval) >> 20,
                rss_in(&ranges.program) >> 20,
            );
        }
        if log > 1 {
            stats.sort_by_key(|c| std::cmp::Reverse((c.live + c.freed) * c.slot));
            for c in stats.iter().take(30) {
                eprintln!(
                    "  {:>10} live ({:>5} MiB) {:>10} freed ({:>5} MiB) {:>5} B  {}",
                    c.live,
                    (c.live * c.slot) >> 20,
                    c.freed,
                    (c.freed * c.slot) >> 20,
                    c.slot,
                    c.name
                );
            }
        }
        self.live_bytes = live_bytes;
        if !minor {
            self.major_live_bytes = live_bytes;
            self.force_major = false;
        }
        self.since_gc = 0;
    }

    /// The objects a collection traces from, besides the registered roots. Major:
    /// every program object whose type can hold an evaluator handle (in check mode,
    /// every program object). Minor: the program objects allocated since the last
    /// collection, and the old program and evaluator objects on written cards.
    fn root_objects(&self, check: bool, minor: bool) -> Vec<(usize, TraceFn)> {
        let ranges = ranges();
        let mut roots = Vec::new();
        for class in self.program.iter().flatten() {
            if !class.info.holds_eval && !check {
                continue;
            }
            // Check mode scans the program types that cannot hold an evaluator handle
            // in full, to verify that claim; the barrier never sees them.
            let precise = minor && class.info.holds_eval;
            let (slot, trace) = (class.info.slot as usize, class.info.trace);
            for &block in &class.blocks {
                // SAFETY: a block of this class, owned by this thread.
                let meta = unsafe { ranges.program.meta(block) };
                let base = ranges.program.block_base(block);
                let young = if precise { meta.scanned as usize } else { 0 };
                if precise {
                    let cards = unsafe { ranges.program.cards(block) };
                    for_card_slots(cards, slot, young, |i| roots.push((base + i * slot, trace)));
                }
                roots.extend((young..meta.bump as usize).map(|i| (base + i * slot, trace)));
            }
        }
        if minor {
            for class in self.eval.iter().flatten() {
                let (slot, trace) = (class.info.slot as usize, class.info.trace);
                for &block in &class.blocks {
                    // SAFETY: as above.
                    let meta = unsafe { ranges.eval.meta(block) };
                    let base = ranges.eval.block_base(block);
                    let cards = unsafe { ranges.eval.cards(block) };
                    let marks = unsafe { std::slice::from_raw_parts(meta.bits.add(meta.words as usize), meta.words as usize) };
                    // Old objects only: a young one is traced when something reaches it.
                    for_card_slots(cards, slot, meta.count as usize, |i| {
                        if marks[i / 64] & (1 << (i % 64)) != 0 {
                            roots.push((base + i * slot, trace));
                        }
                    });
                }
            }
        }
        roots
    }

    /// Generational: after a collection every card is clean and every program object old.
    fn clear_cards(&self) {
        let ranges = ranges();
        for class in self.program.iter().flatten() {
            for &block in &class.blocks {
                // SAFETY: a block of this class, owned by this thread.
                let meta = unsafe { ranges.program.meta(block) };
                meta.scanned = meta.bump;
                unsafe { ranges.program.cards(block) }.fill(0);
            }
        }
        for class in self.eval.iter().flatten() {
            for &block in &class.blocks {
                unsafe { ranges.eval.cards(block) }.fill(0);
            }
        }
    }

    /// Marks everything `roots` and the registered roots reach, on `threads` threads.
    fn mark_from(&self, t: &mut Tracer, roots: &[(usize, TraceFn)], threads: usize) {
        if threads == 1 {
            for chunk in roots.chunks(ROOT_CHUNK) {
                for &(addr, trace) in chunk {
                    // SAFETY: a root entry pairs an object with the trace function of its type.
                    unsafe { trace(addr, t) };
                }
                t.drain();
            }
            for root in &self.roots {
                root(t);
                t.drain();
            }
            return;
        }
        let shared = MarkShared {
            roots: roots.to_vec(),
            next_root: AtomicUsize::new(0),
            pool: Mutex::new(Pool::default()),
            wake: std::sync::Condvar::new(),
            pooled: AtomicUsize::new(0),
            hungry: AtomicUsize::new(0),
            threads,
            abort: AtomicBool::new(false),
            panic: Mutex::new(None),
        };
        t.shared = &shared;
        // The registered roots trace thread-locals, so they run on this thread.
        let registered = catch_unwind(AssertUnwindSafe(|| {
            for root in &self.roots {
                root(t);
            }
        }));
        if let Err(panic) = registered {
            t.shared = ptr::null();
            resume_unwind(panic);
        }
        let job = MarkJob {
            shared: &shared as *const MarkShared as usize,
            owner: t.owner,
            check: t.check,
            eval_base: t.eval_base,
            eval_len: t.eval_len,
            meta: t.meta as usize,
        };
        HELPERS.run(threads - 1, job, || mark_work_caught(t, &shared));
        t.shared = ptr::null();
        if let Some(panic) = lock(&shared.panic).take() {
            resume_unwind(panic);
        }
    }

    /// Check mode, minor collection: marks everything again from all roots and
    /// panics on an object the minor mark missed, that is a write the barrier did
    /// not see. Then restores the minor mark, which keeps every old object.
    fn verify_minor(&self, t: &mut Tracer) {
        let minor = self.marks();
        self.clear_marks();
        let roots = self.root_objects(true, false);
        self.mark_from(t, &roots, 1);
        let full = self.marks();
        let mut k = 0;
        for class in self.eval.iter().flatten() {
            for _ in &class.blocks {
                for (w, (&f, &m)) in full[k].iter().zip(&minor[k]).enumerate() {
                    if f & !m != 0 {
                        let i = w * 64 + (f & !m).trailing_zeros() as usize;
                        panic!(
                            "collector check: a minor collection missed a reachable {} (slot {i}): a write the barrier did not see",
                            class.info.name
                        );
                    }
                }
                k += 1;
            }
        }
        self.set_marks(&minor);
    }

    /// The mark bits of every evaluator block, in class and block order.
    fn marks(&self) -> Vec<Vec<u64>> {
        let eval = &ranges().eval;
        let mut marks = Vec::new();
        for class in self.eval.iter().flatten() {
            for &block in &class.blocks {
                // SAFETY: a block of this class, owned by this thread, with its bitmaps.
                let meta = unsafe { eval.meta(block) };
                let words = meta.words as usize;
                marks.push(unsafe { std::slice::from_raw_parts(meta.bits.add(words), words) }.to_vec());
            }
        }
        marks
    }

    fn set_marks(&self, marks: &[Vec<u64>]) {
        let eval = &ranges().eval;
        let mut k = 0;
        for class in self.eval.iter().flatten() {
            for &block in &class.blocks {
                // SAFETY: as in `marks`; `marks` came from the same blocks.
                let meta = unsafe { eval.meta(block) };
                let words = meta.words as usize;
                unsafe { std::slice::from_raw_parts_mut(meta.bits.add(words), words) }.copy_from_slice(&marks[k]);
                k += 1;
            }
        }
    }

    fn compare_marks(&self, expected: &[Vec<u64>], what: &str) {
        let mut k = 0;
        let actual = self.marks();
        for class in self.eval.iter().flatten() {
            for _ in &class.blocks {
                assert!(actual[k] == expected[k], "collector check: {what} in a block of {}", class.info.name);
                k += 1;
            }
        }
    }

    fn clear_marks(&self) {
        let eval = &ranges().eval;
        for class in self.eval.iter().flatten() {
            for &block in &class.blocks {
                let meta = unsafe { eval.meta(block) };
                let words = meta.words as usize;
                unsafe { ptr::write_bytes(meta.bits.add(words), 0, words) };
            }
        }
    }

    /// Drops every live, unmarked evaluator object. Returns the objects freed and the
    /// slot bytes still live.
    fn sweep(&mut self, check: bool, sticky: bool, mut stats: Option<&mut Vec<ClassStat>>) -> (usize, usize) {
        let eval = &ranges().eval;
        let (mut freed, mut live_bytes) = (0usize, 0usize);
        for class in self.eval.iter_mut().flatten() {
            class.partial.clear();
            class.current = NONE;
            class.free = 0;
            let slot = class.info.slot as usize;
            let mut kept = Vec::with_capacity(class.blocks.len());
            let (freed_before, live_before) = (freed, live_bytes);
            for &block in &class.blocks {
                let base = eval.block_base(block);
                let meta = unsafe { eval.meta(block) };
                let words = meta.words as usize;
                let mut live = 0usize;
                for w in 0..words {
                    unsafe {
                        let marked = *meta.bits.add(words + w);
                        let mut dead = *meta.bits.add(w) & !marked;
                        // Prefetch the buffers of the dead object `SWEEP_AHEAD` after the one dropped.
                        let mut ahead = dead;
                        for _ in 0..SWEEP_AHEAD {
                            if ahead != 0 {
                                (class.info.prefetch_owned)(base + (w * 64 + ahead.trailing_zeros() as usize) * slot);
                                ahead &= ahead - 1;
                            }
                        }
                        while dead != 0 {
                            let j = dead.trailing_zeros() as usize;
                            dead &= dead - 1;
                            if ahead != 0 {
                                (class.info.prefetch_owned)(base + (w * 64 + ahead.trailing_zeros() as usize) * slot);
                                ahead &= ahead - 1;
                            }
                            let i = w * 64 + j;
                            let addr = base + i * slot;
                            (class.info.drop)(addr);
                            if check {
                                ptr::write_bytes(addr as *mut u8, POISON, slot);
                                *meta.bits.add(2 * words + w) |= 1 << j;
                            }
                            freed += 1;
                        }
                        *meta.bits.add(w) = marked;
                        // Generational: a survivor stays marked, which makes it old.
                        *meta.bits.add(words + w) = if sticky { marked } else { 0 };
                        live += marked.count_ones() as usize;
                    }
                }
                live_bytes += live * slot;
                if live == 0 && !check {
                    meta.owner = 0;
                    eval.release_block(block);
                    continue;
                }
                kept.push(block);
                if (0..words).any(|w| unsafe { meta.free_bits(w) } != 0) {
                    class.partial.push(block);
                }
            }
            class.blocks = kept;
            if let Some(stats) = stats.as_deref_mut() {
                let (name, slot) = (class.info.name, slot);
                stats.push(ClassStat { name, slot, live: (live_bytes - live_before) / slot, freed: freed - freed_before });
            }
        }
        (freed, live_bytes)
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::cell::Cell;

    use super::*;
    use crate::P;

    /// The heap tests share the process's pool of released blocks, so one test can
    /// claim a block another test expects to reuse. They run one at a time.
    static POOL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        POOL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[derive(jsrt::Trace)]
    #[trace(eval)]
    struct Node {
        next: Cell<Option<P<Node>>>,
        drops: Rc<()>,
        id: u32,
    }

    #[derive(jsrt::Trace)]
    struct Holder {
        node: Cell<Option<P<Node>>>,
    }

    fn node(next: Option<P<Node>>, drops: &Rc<()>, id: u32) -> P<Node> {
        P::new_eval(Node { next: Cell::new(next), drops: drops.clone(), id })
    }

    #[test]
    fn frees_what_no_root_reaches() {
        let _serial = serial();
        let drops = Rc::new(());
        let root = P::new(Holder { node: Cell::new(None) });
        let mut chain = None;
        for id in 0..1000 {
            chain = Some(node(chain, &drops, id));
            node(None, &drops, id);
        }
        root.node.set(chain);
        assert_eq!(Rc::strong_count(&drops), 2001);

        collect(false);
        assert_eq!(Rc::strong_count(&drops), 1001);
        let (mut n, mut ids) = (root.node.get(), Vec::new());
        while let Some(x) = n {
            ids.push(x.id);
            n = x.next.get();
        }
        assert_eq!(ids, (0..1000).rev().collect::<Vec<_>>());
        assert_eq!(eval_heap_live_bytes(), 1000 * size_of::<Node>().next_multiple_of(8));

        root.node.set(None);
        collect(false);
        assert_eq!(Rc::strong_count(&drops), 1);
        assert_eq!(eval_heap_live_bytes(), 0);
    }

    #[test]
    fn reuses_swept_slots() {
        let _serial = serial();
        let drops = Rc::new(());
        let garbage = node(None, &drops, 0);
        let addr = garbage.addr_for_test();
        collect(false);
        assert_eq!(Rc::strong_count(&drops), 1);
        let mut reused = false;
        for id in 0..BLOCK {
            reused |= node(None, &drops, id as u32).addr_for_test() == addr;
        }
        assert!(reused);
    }

    thread_local! {
        static STACK: crate::cell::RefCell<Vec<P<Node>>> = {
            add_root(|t| STACK.with(|s| s.trace(t)));
            crate::cell::RefCell::new(Vec::new())
        };
    }

    #[test]
    fn traces_registered_roots() {
        let _serial = serial();
        let drops = Rc::new(());
        STACK.with(|s| s.borrow_mut().push(node(None, &drops, 7)));
        collect(false);
        assert_eq!(Rc::strong_count(&drops), 2);
        STACK.with(|s| assert_eq!(s.borrow()[0].id, 7));
    }

    #[test]
    #[should_panic(expected = "use of a swept")]
    fn check_mode_panics_on_use_of_a_swept_object() {
        let _serial = serial();
        let drops = Rc::new(());
        let garbage = node(None, &drops, 3);
        collect(true);
        assert_eq!(Rc::strong_count(&drops), 1);
        let _ = garbage.id;
    }

    #[test]
    fn check_mode_never_reuses() {
        let _serial = serial();
        let drops = Rc::new(());
        let addr = node(None, &drops, 0).addr_for_test();
        collect(true);
        for id in 0..BLOCK {
            assert_ne!(node(None, &drops, id as u32).addr_for_test(), addr);
        }
    }

    #[derive(jsrt::Trace)]
    struct Claim {
        #[trace(program)]
        node: Cell<Option<P<Node>>>,
    }

    #[test]
    #[should_panic(expected = "#[trace(program)] field holds an evaluator handle")]
    fn check_mode_verifies_program_fields() {
        let _serial = serial();
        collect(true);
        let drops = Rc::new(());
        let _claim = P::new(Claim { node: Cell::new(Some(node(None, &drops, 0))) });
        collect(true);
    }

    #[derive(jsrt::Trace)]
    #[trace(eval)]
    struct Cyclic {
        next: Cell<Option<P<Cyclic>>>,
        extra: crate::cell::RefCell<Option<P<Cyclic>>>,
        val: u32,
    }

    #[derive(jsrt::Trace)]
    struct CyclicRoots {
        a: Cell<Option<P<Cyclic>>>,
        c: Cell<Option<P<Cyclic>>>,
    }

    fn cyclic(next: Option<P<Cyclic>>, val: u32) -> P<Cyclic> {
        P::new_eval(Cyclic { next: Cell::new(next), extra: crate::cell::RefCell::new(None), val })
    }

    /// A panic while marking clears the marks it set; otherwise the next collection
    /// would skip `a`'s children and free `b` while `a` still holds it.
    #[test]
    fn unwound_collection_leaves_no_marks() {
        let _serial = serial();
        let b = cyclic(None, 1);
        let a = cyclic(Some(b), 2);
        let c = cyclic(None, 3);
        let roots = P::new(CyclicRoots { a: Cell::new(Some(a)), c: Cell::new(Some(c)) });
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _borrowed = c.extra.borrow_mut();
            collect(false);
        }));
        assert!(unwound.is_err());
        collect(false);
        for id in 0..BLOCK as u32 {
            cyclic(None, 1_000_000 + id);
        }
        assert_eq!(roots.a.get().unwrap().next.get().unwrap().val, 1);
        assert_eq!(roots.c.get().unwrap().val, 3);
    }

    #[derive(jsrt::Trace)]
    #[trace(eval)]
    struct Huge([u8; MAX_SLOT + 1]);

    #[derive(jsrt::Trace)]
    #[trace(eval)]
    struct Fresh(u8);

    /// An oversize type panics before taking the registry lock, so other types still allocate.
    #[test]
    fn oversize_type_leaves_the_registry_usable() {
        let _serial = serial();
        assert!(std::panic::catch_unwind(|| P::new_eval(Huge([0; MAX_SLOT + 1]))).is_err());
        assert!(std::thread::spawn(|| P::new_eval(Fresh(1)).0).join().is_ok());
        assert_eq!(P::new_eval(Fresh(2)).0, 2);
    }

    #[derive(jsrt::Trace)]
    #[trace(eval)]
    struct Big {
        _pad: [u64; 512],
    }

    #[derive(jsrt::Trace)]
    struct Kept {
        v: crate::cell::RefCell<Vec<P<Node>>>,
    }

    /// Blocks another thread released are reused by a type with another slot size,
    /// and survivors stay intact through two more sweeps.
    #[test]
    fn released_blocks_change_type_and_thread() {
        let _serial = serial();
        let released: Vec<usize> = std::thread::spawn(|| {
            let blocks = (0..64).map(|_| P::new_eval(Big { _pad: [0; 512] }).addr_for_test() & !(BLOCK - 1)).collect();
            collect(false);
            assert_eq!(eval_heap_live_bytes(), 0);
            blocks
        })
        .join()
        .unwrap();
        let drops = Rc::new(());
        let kept = P::new(Kept { v: crate::cell::RefCell::new(Vec::new()) });
        let mut reused = 0;
        for id in 0..40_000u32 {
            let n = node(None, &drops, id);
            reused += usize::from(released.contains(&(n.addr_for_test() & !(BLOCK - 1))));
            if id % 2 == 0 {
                kept.v.borrow_mut().push(n);
            }
        }
        assert!(reused > 0);
        collect(false);
        assert_eq!(Rc::strong_count(&drops), 1 + 20_000);
        kept.v.borrow_mut().truncate(5);
        collect(false);
        assert_eq!(Rc::strong_count(&drops), 1 + 5);
        for id in 0..40_000u32 {
            node(None, &drops, 9_000_000 + id);
        }
        for (k, n) in kept.v.borrow().iter().enumerate() {
            assert_eq!(n.id, 2 * k as u32);
        }
    }

    /// A parallel mark keeps what a serial one keeps, on a graph large enough to
    /// spread over the helpers: chains hang off many program roots, half of them cut.
    #[test]
    fn parallel_mark_matches_serial() {
        let _serial = serial();
        let drops = Rc::new(());
        let roots: Vec<_> = (0..2000).map(|_| P::new(Holder { node: Cell::new(None) })).collect();
        for (r, root) in roots.iter().enumerate() {
            let mut chain = None;
            for id in 0..100 {
                chain = Some(node(chain, &drops, id));
            }
            if r % 2 == 0 {
                root.node.set(chain);
            }
        }
        with_heap(|heap| heap.collect(Collection { check: false, log: 0, threads: 4, minor: false, generational: false }));
        assert_eq!(Rc::strong_count(&drops), 1 + 1000 * 100);
        for root in roots.iter().step_by(2) {
            root.node.set(root.node.get().unwrap().next.get());
        }
        with_heap(|heap| heap.collect(Collection { check: false, log: 0, threads: 1, minor: false, generational: false }));
        assert_eq!(Rc::strong_count(&drops), 1 + 1000 * 99);
        for root in &roots {
            root.node.set(None);
        }
        with_heap(|heap| heap.collect(Collection { check: false, log: 0, threads: 4, minor: false, generational: false }));
        assert_eq!(Rc::strong_count(&drops), 1);
    }

    fn generational(minor: bool, check: bool) -> Collection {
        Collection { check, log: 0, threads: 4, minor, generational: true }
    }

    /// Turns the write barrier on, as generational collection does at start.
    fn enable_barrier() {
        let ranges = ranges();
        BARRIER.eval_base.store(ranges.eval.base, Relaxed);
        BARRIER.eval_cards.store(ranges.eval.cards, Relaxed);
        BARRIER.eval_len.store(ranges.eval.len, Relaxed);
        BARRIER.program_base.store(ranges.program.base, Relaxed);
        BARRIER.program_cards.store(ranges.program.cards, Relaxed);
        BARRIER.program_len.store(ranges.program.len, Relaxed);
    }

    /// A minor collection frees young garbage, keeps young objects that an old one
    /// was given through the barrier, and keeps old garbage until a major collection.
    #[test]
    fn minor_collection_keeps_old_objects_and_barriered_young_ones() {
        let _serial = serial();
        enable_barrier();
        let drops = Rc::new(());
        let root = P::new(Holder { node: Cell::new(None) });
        let other = P::new(Holder { node: Cell::new(None) });
        let old = node(None, &drops, 1);
        root.node.set(Some(old));
        other.node.set(Some(node(None, &drops, 2)));
        with_heap(|heap| heap.collect(generational(false, false)));
        assert_eq!(Rc::strong_count(&drops), 3);

        // Both are old now. Cut one loose: it stays until a major collection. Give
        // the other a young child through the barrier, next to young garbage.
        other.node.set(None);
        let young = node(None, &drops, 3);
        old.next.set(Some(young));
        node(None, &drops, 4);
        with_heap(|heap| heap.collect(generational(true, false)));
        assert_eq!(Rc::strong_count(&drops), 4, "young garbage freed, old garbage kept");
        assert_eq!(root.node.get().unwrap().next.get().unwrap().id, 3);

        // A young program object is a root at the next minor collection.
        let fresh = P::new(Holder { node: Cell::new(Some(node(None, &drops, 5))) });
        with_heap(|heap| heap.collect(generational(true, false)));
        assert_eq!(fresh.node.get().unwrap().id, 5);
        assert_eq!(Rc::strong_count(&drops), 5);

        with_heap(|heap| heap.collect(generational(false, false)));
        assert_eq!(Rc::strong_count(&drops), 4, "the major collection frees old garbage");
        root.node.set(None);
        fresh.node.set(None);
        with_heap(|heap| heap.collect(generational(false, false)));
        assert_eq!(Rc::strong_count(&drops), 1);
    }

    /// Check mode re-marks after a minor collection and panics on a young object
    /// that an old one reached through a write the barrier did not see.
    #[test]
    fn check_mode_catches_a_write_the_barrier_missed() {
        let _serial = serial();
        enable_barrier();
        let drops = Rc::new(());
        let root = P::new(Holder { node: Cell::new(None) });
        let old = node(None, &drops, 1);
        root.node.set(Some(old));
        with_heap(|heap| heap.collect(generational(false, true)));
        let young = node(None, &drops, 2);
        // SAFETY: a test of the check: writes past the barrier, as a cell without one would.
        unsafe { ptr::write(ptr::from_ref(&old.next).cast_mut().cast::<Option<P<Node>>>(), Some(young)) };
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| with_heap(|heap| heap.collect(generational(true, true)))));
        let message = unwound.expect_err("check mode missed the unseen write");
        let message = message.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(message.contains("a write the barrier did not see"), "{message}");
        root.node.set(None);
        with_heap(|heap| heap.collect(generational(false, true)));
    }

    #[derive(jsrt::Trace)]
    #[trace(eval)]
    struct AllocatesWhileTraced;

    struct AllocatingRoot(AllocatesWhileTraced);

    // SAFETY: a test of the re-entry guard; it holds no handle.
    unsafe impl Trace for AllocatingRoot {
        const HOLDS_EVAL: bool = true;
        fn trace(&self, _: &mut Tracer) {
            P::new_eval(Fresh(0));
        }
    }

    #[test]
    #[should_panic(expected = "the heap was used inside an allocation or a collection")]
    fn allocation_inside_a_collection_panics() {
        let _serial = serial();
        let _root = P::new(AllocatingRoot(AllocatesWhileTraced));
        collect(false);
    }

    /// Whichever thread traces the root, the allocation panics.
    #[test]
    fn allocation_inside_a_parallel_mark_panics() {
        let _serial = serial();
        let roots: Vec<_> = (0..4096).map(|_| P::new(Holder { node: Cell::new(None) })).collect();
        let _root = P::new(AllocatingRoot(AllocatesWhileTraced));
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| with_heap(|heap| heap.collect(Collection { check: false, log: 0, threads: 4, minor: false, generational: false }))));
        let message = unwound.expect_err("the allocation did not panic");
        let message = message.downcast_ref::<String>().map(String::as_str).or(message.downcast_ref::<&str>().copied());
        assert_eq!(message, Some("collector: the heap was used inside an allocation or a collection"));
        drop(roots);
    }
}
