//! Typed-block heaps behind `P`, and the evaluator heap collector
//! (`docs/collector-design.md`).
//!
//! Two address ranges are reserved once per process and cut into 64 KiB
//! blocks. A block belongs to one thread and holds objects of one Rust type in
//! fixed slots; its metadata sits in a side table indexed by block number.
//! Program objects whose type can hold no evaluator handle stay in a bump
//! arena (`arena.rs`): the collector never needs to enumerate them.

use std::any::TypeId;
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use crate::collections::FxHashMap;
use crate::trace::Trace;

const BLOCK_SHIFT: u32 = 16;
const BLOCK: usize = 1 << BLOCK_SHIFT;
/// Slot sizes up to this keep the reciprocal slot index exact (offset × slot < 2^32).
const MAX_SLOT: usize = 1 << 14;
const NONE: u32 = u32::MAX;
const PROGRAM_RESERVE: usize = 64 << 30;
const EVAL_RESERVE: usize = 256 << 30;
const POISON: u8 = 0xDB;

/// `PYRIGHT_RS_GC`: whether the evaluator heap is collected between files.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GcMode {
    Off,
    On,
    /// Collect often; poison swept objects, never reuse them, and panic on use.
    Check,
}

struct Config {
    mode: GcMode,
    min_bytes: usize,
    log: bool,
}

static CONFIG: OnceLock<Config> = OnceLock::new();
/// Check mode's hooks on `P` and on program allocation exist only in a build
/// with `--features jsrt/gc-check`: a flag read on every dereference costs
/// about 5% wall time on the samples corpus.
const CHECKS: bool = cfg!(any(test, feature = "gc-check"));
/// Read by every `P` dereference in a `gc-check` build: true in check mode.
static CHECK_HANDLES: AtomicBool = AtomicBool::new(false);

fn config() -> &'static Config {
    CONFIG.get_or_init(|| {
        let mode = match std::env::var("PYRIGHT_RS_GC").as_deref() {
            Err(_) | Ok("") | Ok("off") => GcMode::Off,
            Ok("on") => GcMode::On,
            Ok("check") => GcMode::Check,
            Ok(other) => panic!("PYRIGHT_RS_GC must be off, on or check, not {other:?}"),
        };
        let default_min_mib = if mode == GcMode::Check { 64 } else { 1024 };
        let min_mib = std::env::var("PYRIGHT_RS_GC_MIN_MIB").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(default_min_mib);
        if mode == GcMode::Check {
            if !CHECKS {
                panic!("PYRIGHT_RS_GC=check needs a build with --features jsrt/gc-check");
            }
            CHECK_HANDLES.store(true, Relaxed);
        }
        Config { mode, min_bytes: min_mib << 20, log: std::env::var_os("PYRIGHT_RS_GC_LOG").is_some() }
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

struct Range {
    name: &'static str,
    base: usize,
    len: usize,
    meta: *mut BlockMeta,
    next: AtomicUsize,
    free: Mutex<Vec<u32>>,
}

// SAFETY: a block's metadata is touched only by the thread that owns the block;
// `next` and `free` hand blocks out across threads.
unsafe impl Sync for Range {}
unsafe impl Send for Range {}

impl Range {
    fn reserve(name: &'static str, len: usize) -> Range {
        let base = map(len + BLOCK).next_multiple_of(BLOCK);
        let meta = map((len >> BLOCK_SHIFT) * size_of::<BlockMeta>()) as *mut BlockMeta;
        Range { name, base, len, meta, next: AtomicUsize::new(0), free: Mutex::new(Vec::new()) }
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
}

/// Reserves address space; pages are committed on first touch.
fn map(len: usize) -> usize {
    // SAFETY: an anonymous private mapping at an address the kernel picks.
    let p = unsafe {
        libc::mmap(
            ptr::null_mut(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
            -1,
            0,
        )
    };
    assert!(p != libc::MAP_FAILED, "collector: cannot reserve {} GiB of address space", len >> 30);
    p as usize
}

struct Ranges {
    program: Range,
    eval: Range,
}

static RANGES: OnceLock<Ranges> = OnceLock::new();

fn ranges() -> &'static Ranges {
    RANGES.get_or_init(|| Ranges {
        program: Range::reserve("program", PROGRAM_RESERVE),
        eval: Range::reserve("evaluator", EVAL_RESERVE),
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
    let mut registry = REGISTRY.lock().unwrap();
    let registry = registry.get_or_insert_with(Registry::default);
    if let Some(&id) = registry.ids.get(&TypeId::of::<T>()) {
        return id;
    }
    let name = std::any::type_name::<T>();
    let slot = size_of::<T>().max(8).next_multiple_of(align_of::<T>());
    assert!(slot <= MAX_SLOT, "collector: {name} is {slot} bytes; slots go up to {MAX_SLOT}");
    let id = registry.infos.len() as u32;
    registry.infos.push(TypeInfo {
        name,
        holds_eval: T::HOLDS_EVAL,
        slot: slot as u32,
        recip: (1u64 << 32).div_ceil(slot as u64) as u32,
        count: (BLOCK / slot) as u32,
        drop: drop_erased::<T>,
        trace: trace_erased::<T>,
    });
    registry.ids.insert(TypeId::of::<T>(), id);
    id
}

fn type_info(id: u32) -> TypeInfo {
    REGISTRY.lock().unwrap().as_ref().unwrap().infos[id as usize]
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

static NEXT_OWNER: AtomicU32 = AtomicU32::new(1);

thread_local! {
    static HEAP: &'static UnsafeCell<ThreadHeap> = Box::leak(Box::new(UnsafeCell::new(ThreadHeap {
        owner: NEXT_OWNER.fetch_add(1, Relaxed),
        generic: FxHashMap::default(),
        program: Vec::new(),
        eval: Vec::new(),
        roots: Vec::new(),
        since_gc: 0,
        live_bytes: 0,
        collections: 0,
    })));
}

/// Runs `f` on this thread's heap. Nothing `f` calls may re-enter the heap.
#[inline]
fn with_heap<R>(f: impl FnOnce(&mut ThreadHeap) -> R) -> R {
    // SAFETY: allocation, collection and root registration never call back into user code
    // that allocates, so no two `&mut` to the heap coexist.
    HEAP.with(|h| f(unsafe { &mut *h.get() }))
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
    let block = range.free.lock().unwrap().pop().unwrap_or_else(|| {
        let block = range.next.fetch_add(1, Relaxed);
        assert!(block < range.len >> BLOCK_SHIFT, "collector: the {} heap reservation of {} GiB is full", range.name, range.len >> 30);
        block as u32
    });
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
    // SAFETY: a handle into the evaluator range points into a block this thread owns.
    let meta = unsafe { range.meta((offset >> BLOCK_SHIFT) as u32) };
    if meta.bits.is_null() {
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

/// Collects when the evaluator heap grew by `max(live bytes, floor)` since the
/// last collection (by the floor alone in check mode), unless `PYRIGHT_RS_GC`
/// is off. Call only where no evaluator frame is live: before a file in the
/// check loop.
pub fn collect_if_due() -> bool {
    let config = config();
    if config.mode == GcMode::Off {
        return false;
    }
    let check = config.mode == GcMode::Check;
    with_heap(|heap| {
        let threshold = if check { config.min_bytes } else { heap.live_bytes.max(config.min_bytes) };
        if heap.since_gc == 0 || heap.since_gc < threshold {
            return false;
        }
        unsafe { heap.collect(check, config.log) };
        true
    })
}

/// Collects now, in check mode when `check`. Same precondition as [`collect_if_due`].
pub fn collect(check: bool) {
    if check {
        CHECK_HANDLES.store(true, Relaxed);
    }
    let log = config().log;
    with_heap(|heap| unsafe { heap.collect(check, log) });
}

impl ThreadHeap {
    unsafe fn collect(&mut self, check: bool, log: bool) {
        let start = Instant::now();
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

        // The program heap is never collected, so each of its objects is a root.
        let mut scanned = 0usize;
        for class in self.program.iter().flatten() {
            if !class.info.holds_eval && !check {
                continue;
            }
            let slot = class.info.slot as usize;
            for &block in &class.blocks {
                let base = ranges.program.block_base(block);
                let count = unsafe { ranges.program.meta(block).bump } as usize;
                for i in 0..count {
                    unsafe { (class.info.trace)(base + i * slot, &mut t) };
                }
                scanned += count;
                t.drain();
            }
        }
        for root in self.roots.clone() {
            root(&mut t);
            t.drain();
        }

        let mark_ms = start.elapsed().as_millis();
        let (mut freed, mut live_bytes) = (0usize, 0usize);
        for class in self.eval.iter_mut().flatten() {
            class.partial.clear();
            class.current = NONE;
            let slot = class.info.slot as usize;
            let mut kept = Vec::with_capacity(class.blocks.len());
            for &block in &class.blocks {
                let base = ranges.eval.block_base(block);
                let meta = unsafe { ranges.eval.meta(block) };
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
                    unsafe { libc::madvise(base as *mut libc::c_void, BLOCK, libc::MADV_DONTNEED) };
                    meta.owner = 0;
                    ranges.eval.free.lock().unwrap().push(block);
                    continue;
                }
                kept.push(block);
                if meta.free != NONE || meta.bump < meta.count {
                    class.partial.push(block);
                }
            }
            class.blocks = kept;
        }

        self.collections += 1;
        if log {
            eprintln!(
                "gc {}: {} ms ({} ms mark), {} program objects scanned, {} objects freed, evaluator slots {} MiB live (+{} MiB allocated since the last collection)",
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
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::P;

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
        let drops = Rc::new(());
        STACK.with(|s| s.borrow_mut().push(node(None, &drops, 7)));
        collect(false);
        assert_eq!(Rc::strong_count(&drops), 2);
        STACK.with(|s| assert_eq!(s.borrow()[0].id, 7));
    }

    #[test]
    #[should_panic(expected = "use of a swept")]
    fn check_mode_panics_on_use_of_a_swept_object() {
        let drops = Rc::new(());
        let garbage = node(None, &drops, 3);
        collect(true);
        assert_eq!(Rc::strong_count(&drops), 1);
        let _ = garbage.id;
    }

    #[test]
    fn check_mode_never_reuses() {
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
        collect(true);
        let drops = Rc::new(());
        let _claim = P::new(Claim { node: Cell::new(Some(node(None, &drops, 0))) });
        collect(true);
    }
}
