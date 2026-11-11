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
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
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
    log: bool,
}

static CONFIG: OnceLock<Config> = OnceLock::new();
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
        Config { mode, min_bytes: min_mib << 20, ratio, log: std::env::var_os("PYRIGHT_RS_GC_LOG").is_some() }
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
    bump: u32,
    free: u32,
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

    /// One slot, from the free list or by bumping. Sets its live bit on the evaluator heap.
    #[inline]
    unsafe fn take(&mut self, base: usize, eval: bool) -> Option<usize> {
        let slot = self.slot as usize;
        let i = if self.free != NONE {
            let i = self.free;
            self.free = unsafe { ptr::read_unaligned((base + i as usize * slot) as *const u32) };
            i
        } else if self.bump < self.count {
            self.bump += 1;
            self.bump - 1
        } else {
            return None;
        };
        if eval {
            unsafe { *self.bits.add(i as usize / 64) |= 1 << (i % 64) };
        }
        Some(base + i as usize * slot)
    }
}

/// Blocks handed out so far, blocks committed, and released blocks.
struct RangeBlocks {
    next: usize,
    committed: usize,
    free: Vec<u32>,
}

struct Range {
    name: &'static str,
    base: usize,
    len: usize,
    meta: *mut BlockMeta,
    blocks: Mutex<RangeBlocks>,
}

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
                    let base = base.next_multiple_of(BLOCK);
                    let blocks = Mutex::new(RangeBlocks { next: 0, committed: 0, free: Vec::new() });
                    return Range { name, base, len, meta: meta as *mut BlockMeta, blocks };
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

    /// A free block: a released one, or the next one, committing more when needed.
    fn take_block(&self) -> u32 {
        let mut blocks = lock(&self.blocks);
        if let Some(block) = blocks.free.pop() {
            return block;
        }
        let block = blocks.next;
        if block == blocks.committed {
            let count = COMMIT_BLOCKS.min((self.len >> BLOCK_SHIFT) - block);
            let committed = count > 0
                && protect(self.block_base(block as u32), count << BLOCK_SHIFT)
                && protect(self.meta as usize + block * size_of::<BlockMeta>(), count * size_of::<BlockMeta>());
            if !committed {
                drop(blocks);
                panic!("collector: the {} heap reservation of {} MiB is full or cannot be committed", self.name, self.len >> 20);
            }
            blocks.committed += count;
        }
        blocks.next += 1;
        block as u32
    }

    fn release_block(&self, block: u32) {
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

fn ranges() -> &'static Ranges {
    RANGES.get_or_init(|| {
        // Under an address-space limit, leave most of it to the program heap's bump
        // arena, the system allocator and the stack.
        let limit = address_space_limit();
        Ranges {
            program: Range::reserve("program", PROGRAM_RESERVE, limit / 16),
            eval: Range::reserve("evaluator", EVAL_RESERVE, limit / 4),
        }
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
        let classes = if eval { &mut self.eval } else { &mut self.program };
        if let Some(Some(class)) = classes.get_mut(id)
            && class.current != NONE
        {
            let range = if eval { &ranges().eval } else { &ranges().program };
            let base = range.block_base(class.current);
            if let Some(addr) = unsafe { range.meta(class.current).take(base, eval) } {
                if eval {
                    self.since_gc += class.info.slot as usize;
                }
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
            blocks: Vec::new(),
            partial: Vec::new(),
        });
        loop {
            if class.current != NONE
                && let Some(addr) = unsafe { range.meta(class.current).take(range.block_base(class.current), eval) }
            {
                if eval {
                    self.since_gc += class.info.slot as usize;
                }
                return addr;
            }
            class.current = match class.partial.pop() {
                Some(block) => block,
                None => unsafe { new_block(range, class, owner, eval) },
            };
        }
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
    meta.free = NONE;
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

    fn mark_eval(&mut self, offset: usize) {
        assert!(self.program_only == 0, "collector check: a #[trace(program)] field holds an evaluator handle");
        // SAFETY: checked below that this thread owns the block.
        let meta = unsafe { &mut *self.meta.add(offset >> BLOCK_SHIFT) };
        assert!(meta.owner == self.owner, "collector: a handle points into an evaluator block this thread does not own");
        let i = meta.index(offset & (BLOCK - 1));
        let (w, bit, words) = (i / 64, 1u64 << (i % 64), meta.words as usize);
        unsafe {
            if *meta.bits.add(w) & bit == 0 {
                let state = if *meta.bits.add(2 * words + w) & bit != 0 { "swept" } else { "free" };
                panic!("collector: a reachable handle points to a {state} {}", meta.name);
            }
            let mark = meta.bits.add(words + w);
            if *mark & bit != 0 {
                return;
            }
            *mark |= bit;
        }
        let block_base = self.eval_base + (offset & !(BLOCK - 1));
        self.stack.push((block_base + i * meta.slot as usize, meta.trace));
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

    fn drain(&mut self) {
        while let Some((addr, trace)) = self.stack.pop() {
            // SAFETY: `addr` is a live slot of the type `trace` was made for.
            unsafe { trace(addr, self) };
        }
    }
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
        let threshold = ((heap.live_bytes as f64 * config.ratio) as usize).max(config.min_bytes);
        if heap.since_gc == 0 || heap.since_gc < threshold {
            return false;
        }
        heap.collect(check, config.log);
        true
    })
}

/// Collects now, in check mode when `check`. Same precondition as [`collect_if_due`].
pub fn collect(check: bool) {
    if check {
        CHECK_HANDLES.store(true, Relaxed);
    }
    let log = config().log;
    with_heap(|heap| heap.collect(check, log));
}

/// Bytes the system allocator has handed out and not taken back, when it reports them.
fn malloc_in_use() -> Option<usize> {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        // SAFETY: `mallinfo2` only reads allocator statistics.
        let info = unsafe { libc::mallinfo2() };
        Some(info.uordblks + info.hblkhd)
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    None
}

impl ThreadHeap {
    fn collect(&mut self, check: bool, log: bool) {
        let start = Instant::now();
        let malloc_before = if log { malloc_in_use() } else { None };
        let ranges = ranges();
        let mut t = Tracer {
            stack: Vec::new(),
            check,
            program_only: 0,
            owner: self.owner,
            eval_base: ranges.eval.base,
            eval_len: ranges.eval.len,
            meta: ranges.eval.meta,
        };

        // A panic while marking (check mode's assertions, a borrowed RefCell) must not
        // leave mark bits behind: the next collection would skip those objects' children.
        let marked = catch_unwind(AssertUnwindSafe(|| self.mark(&mut t, check)));
        let scanned = match marked {
            Ok(scanned) => scanned,
            Err(panic) => {
                self.clear_marks();
                resume_unwind(panic);
            }
        };
        let mark_ms = start.elapsed().as_millis();

        // A panic while sweeping leaves blocks half-swept; nothing can be trusted after it.
        let swept = catch_unwind(AssertUnwindSafe(|| self.sweep(check)));
        let Ok((freed, live_bytes)) = swept else {
            eprintln!("collector: a drop panicked during the sweep; aborting");
            std::process::abort();
        };

        self.collections += 1;
        if log {
            let malloc = match (malloc_before, malloc_in_use()) {
                (Some(before), Some(after)) => format!(", malloc {} -> {} MiB", before >> 20, after >> 20),
                _ => String::new(),
            };
            eprintln!(
                "gc {}: {} ms ({} ms mark), {} program objects scanned, {} objects freed, evaluator slots {} MiB live (+{} MiB allocated since the last collection){malloc}",
                self.collections,
                start.elapsed().as_millis(),
                mark_ms,
                scanned,
                freed,
                live_bytes >> 20,
                self.since_gc >> 20,
            );
        }
        self.live_bytes = live_bytes;
        self.since_gc = 0;
    }

    /// Marks everything the roots reach. Returns the program objects scanned.
    fn mark(&self, t: &mut Tracer, check: bool) -> usize {
        let program = &ranges().program;
        // The program heap is never collected, so each of its objects is a root.
        let mut scanned = 0usize;
        for class in self.program.iter().flatten() {
            if !class.info.holds_eval && !check {
                continue;
            }
            let slot = class.info.slot as usize;
            for &block in &class.blocks {
                let base = program.block_base(block);
                let count = unsafe { program.meta(block).bump } as usize;
                for i in 0..count {
                    unsafe { (class.info.trace)(base + i * slot, t) };
                }
                scanned += count;
                t.drain();
            }
        }
        for root in &self.roots {
            root(t);
            t.drain();
        }
        scanned
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
    fn sweep(&mut self, check: bool) -> (usize, usize) {
        let eval = &ranges().eval;
        let (mut freed, mut live_bytes) = (0usize, 0usize);
        for class in self.eval.iter_mut().flatten() {
            class.partial.clear();
            class.current = NONE;
            let slot = class.info.slot as usize;
            let mut kept = Vec::with_capacity(class.blocks.len());
            for &block in &class.blocks {
                let base = eval.block_base(block);
                let meta = unsafe { eval.meta(block) };
                let words = meta.words as usize;
                let mut live = 0usize;
                for w in 0..words {
                    unsafe {
                        let marked = *meta.bits.add(words + w);
                        let mut dead = *meta.bits.add(w) & !marked;
                        while dead != 0 {
                            let j = dead.trailing_zeros() as usize;
                            dead &= dead - 1;
                            let i = w * 64 + j;
                            let addr = base + i * slot;
                            (class.info.drop)(addr);
                            if check {
                                ptr::write_bytes(addr as *mut u8, POISON, slot);
                                *meta.bits.add(2 * words + w) |= 1 << j;
                            } else {
                                ptr::write_unaligned(addr as *mut u32, meta.free);
                                meta.free = i as u32;
                            }
                            freed += 1;
                        }
                        *meta.bits.add(w) = marked;
                        *meta.bits.add(words + w) = 0;
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
                if meta.free != NONE || meta.bump < meta.count {
                    class.partial.push(block);
                }
            }
            class.blocks = kept;
        }
        (freed, live_bytes)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

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
        static STACK: std::cell::RefCell<Vec<P<Node>>> = {
            add_root(|t| STACK.with(|s| s.trace(t)));
            std::cell::RefCell::new(Vec::new())
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
        extra: std::cell::RefCell<Option<P<Cyclic>>>,
        val: u32,
    }

    #[derive(jsrt::Trace)]
    struct CyclicRoots {
        a: Cell<Option<P<Cyclic>>>,
        c: Cell<Option<P<Cyclic>>>,
    }

    fn cyclic(next: Option<P<Cyclic>>, val: u32) -> P<Cyclic> {
        P::new_eval(Cyclic { next: Cell::new(next), extra: std::cell::RefCell::new(None), val })
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
        v: std::cell::RefCell<Vec<P<Node>>>,
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
        let kept = P::new(Kept { v: std::cell::RefCell::new(Vec::new()) });
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
}
